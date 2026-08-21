//! ChatGPT official data export (`conversations.json`).
//!
//! The distinguishing feature of this format, and the reason a naive importer
//! loses data: a conversation is not a list of messages. It is a `mapping` of
//! node id → `{ parent, children, message }`, plus a `current_node` pointer at
//! the branch the user last saw. Every regeneration and every edited prompt
//! creates a *sibling* branch. Walking `current_node` up through `parent` and
//! rendering that chain — which is what most importers do, and what
//! ModelCaddy's own `archive_intake` provider does — silently discards every
//! alternative the user generated.
//!
//! This adapter keeps the whole graph and records the active branch separately.

use super::{Adapter, Detection, ExportFile};
use crate::ir::{ContentPart, Conversation, Document, Message, ProjectRef, Role, Source};
use crate::warning::{Severity, WarningCode, Warnings};
use crate::Error;
use serde_json::Value;
use std::collections::HashSet;

pub struct ChatGpt;

const PLATFORM: &str = "chatgpt";
const VARIANT: &str = "official_export_v1";

/// Find the conversations file.
///
/// Filename first, because it is cheap and right almost always. But a user who
/// renamed their export, or unpacked it oddly, must not be told their data is
/// unrecognisable — so fall back to sniffing the shape of every JSON file.
/// Detection must never depend on a filename we do not control.
fn pick_conversations_file(files: &[ExportFile]) -> Option<&ExportFile> {
    let by_name = files.iter().find(|f| {
        let lower = f.lower_path();
        lower.ends_with("conversations.json")
            || (lower.ends_with(".json") && lower.contains("conversation"))
    });
    if by_name.is_some() {
        return by_name;
    }
    files
        .iter()
        .find(|f| f.lower_path().ends_with(".json") && looks_like_chatgpt(&f.bytes))
}

/// Shape test, independent of filename: a JSON array whose first element has a
/// `mapping` object.
fn looks_like_chatgpt(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .as_ref()
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|first| first.get("mapping"))
        .map(Value::is_object)
        .unwrap_or(false)
}

impl Adapter for ChatGpt {
    fn platform(&self) -> &'static str {
        PLATFORM
    }

    fn variant(&self) -> &'static str {
        VARIANT
    }

    fn detect(&self, files: &[ExportFile]) -> Option<Detection> {
        let file = pick_conversations_file(files)?;
        let parsed: Value = serde_json::from_slice(&file.bytes).ok()?;
        let array = parsed.as_array()?;
        let first = array.first()?;
        // Export rows are identifiable by their shape: a `mapping` node graph
        // plus a `current_node` pointer.
        let has_mapping = first.get("mapping").map(Value::is_object).unwrap_or(false);
        let has_current_node = first.get("current_node").is_some();
        let has_title = first.get("title").is_some();
        let confidence = match (has_mapping, has_current_node, has_title) {
            (true, true, _) => 0.98,
            (true, false, true) => 0.85,
            (false, _, true) => 0.40,
            _ => return None,
        };
        Some(Detection {
            platform: PLATFORM,
            variant: VARIANT,
            confidence,
            notes: vec![format!("{} conversation(s)", array.len())],
        })
    }

    fn parse(&self, files: &[ExportFile], warnings: &mut Warnings) -> Result<Document, Error> {
        let file = pick_conversations_file(files)
            .ok_or_else(|| Error::NotRecognized("no conversations.json in export".into()))?;
        let parsed: Value = serde_json::from_slice(&file.bytes)?;
        let array = parsed
            .as_array()
            .ok_or_else(|| Error::Malformed("conversations.json must be a JSON array".into()))?;

        let mut doc = Document::new(Source::new(PLATFORM, VARIANT));
        for (index, conv) in array.iter().enumerate() {
            match parse_conversation(conv, index, warnings) {
                Some(c) => doc.conversations.push(c),
                None => warnings.note(WarningCode::ItemSkipped, Severity::Dropped),
            }
        }
        Ok(doc)
    }
}

fn parse_conversation(conv: &Value, index: usize, warnings: &mut Warnings) -> Option<Conversation> {
    let mapping = conv.get("mapping")?.as_object()?;

    let id = conv
        .get("conversation_id")
        .or_else(|| conv.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            warnings.note(WarningCode::SynthesizedId, Severity::Info);
            format!("chatgpt-conversation-{index}")
        });

    let mut out = Conversation::new(id.clone());
    out.title = conv
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    out.created_at = conv.get("create_time").and_then(unix_to_rfc3339);
    out.updated_at = conv.get("update_time").and_then(unix_to_rfc3339);
    if out.created_at.is_none() && out.updated_at.is_none() {
        warnings.note_for(WarningCode::MissingTimestamps, Severity::Info, &id);
    }
    // Present on conversations that live in a ChatGPT project.
    if let Some(pid) = conv
        .get("conversation_template_id")
        .or_else(|| conv.get("gizmo_id"))
        .and_then(Value::as_str)
    {
        out.project = Some(ProjectRef {
            id: pid.to_string(),
            name: None,
        });
    }

    // Every node with a message becomes a Message, whether or not it sits on
    // the active branch. This is the whole point of the adapter.
    for (node_id, node) in mapping.iter() {
        let Some(msg) = node.get("message") else {
            continue;
        };
        if msg.is_null() {
            continue;
        }
        let role = msg
            .get("author")
            .and_then(|a| a.get("role"))
            .and_then(Value::as_str)
            .map(Role::parse)
            .unwrap_or_else(|| Role::Other("unknown".into()));

        let mut message = Message::new(node_id.clone(), role);
        message.parent = node
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_string);
        message.created_at = msg.get("create_time").and_then(unix_to_rfc3339);
        message.model = msg
            .get("metadata")
            .and_then(|m| m.get("model_slug"))
            .and_then(Value::as_str)
            .map(str::to_string);
        // ChatGPT marks plumbing turns it never showed the user.
        message.hidden = msg
            .get("metadata")
            .and_then(|m| m.get("is_visually_hidden_from_conversation"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || matches!(message.role, Role::System);
        message.content = extract_content(msg, &id, node_id, warnings);
        out.messages.push(message);
    }

    if out.messages.is_empty() {
        return None;
    }
    // Stable order: parents before children where possible, then by id. The
    // graph is authoritative; this only makes output deterministic.
    out.messages.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });

    // The export's root node carries no message, so any parent pointing at it
    // would dangle. A root here means "no parent among the messages", which is
    // what a consumer needs to know.
    let known: HashSet<String> = out.messages.iter().map(|m| m.id.clone()).collect();
    for m in out.messages.iter_mut() {
        if m.parent.as_ref().is_some_and(|p| !known.contains(p)) {
            m.parent = None;
        }
    }

    out.active_path = active_path(conv, mapping, &out, &id, warnings);
    out.raw = Some(conv.clone());
    Some(out)
}

/// Walk `current_node` up through `parent` to the root, then reverse.
fn active_path(
    conv: &Value,
    mapping: &serde_json::Map<String, Value>,
    parsed: &Conversation,
    conversation_id: &str,
    warnings: &mut Warnings,
) -> Vec<String> {
    let Some(start) = conv.get("current_node").and_then(Value::as_str) else {
        warnings.note_for(
            WarningCode::BranchPointerBroken,
            Severity::Lossy,
            conversation_id,
        );
        return Vec::new();
    };

    let known: HashSet<&str> = parsed.messages.iter().map(|m| m.id.as_str()).collect();
    let mut chain: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut cursor = Some(start);

    while let Some(node_id) = cursor {
        if !seen.insert(node_id) {
            warnings.note_for(WarningCode::BranchCycle, Severity::Lossy, conversation_id);
            break;
        }
        if known.contains(node_id) {
            chain.push(node_id.to_string());
        }
        cursor = mapping
            .get(node_id)
            .and_then(|n| n.get("parent"))
            .and_then(Value::as_str);
    }

    if chain.is_empty() {
        warnings.note_for(
            WarningCode::BranchPointerBroken,
            Severity::Lossy,
            conversation_id,
        );
    }
    chain.reverse();
    chain
}

/// ChatGPT stores content as `{ content_type, parts: [...] }`. Parts are
/// strings for plain text and objects for everything else. An object part is
/// preserved verbatim rather than replaced with a placeholder.
fn extract_content(
    msg: &Value,
    conversation_id: &str,
    message_id: &str,
    warnings: &mut Warnings,
) -> Vec<ContentPart> {
    let Some(content) = msg.get("content") else {
        return Vec::new();
    };
    let content_type = content
        .get("content_type")
        .and_then(Value::as_str)
        .unwrap_or("text");

    let Some(parts) = content.get("parts").and_then(Value::as_array) else {
        // Some message kinds use `{ content: { text: "…" } }` instead.
        if let Some(text) = content.get("text").and_then(Value::as_str) {
            return vec![ContentPart::Text {
                text: text.to_string(),
            }];
        }
        warnings.push(
            crate::warning::Warning::new(WarningCode::UnknownContentPart, Severity::Lossy)
                .for_conversation(conversation_id)
                .with_detail(format!("content_type={content_type}")),
        );
        return vec![ContentPart::Unknown {
            kind: Some(content_type.to_string()),
            raw: content.clone(),
        }];
    };

    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        if let Some(s) = part.as_str() {
            if s.is_empty() {
                continue;
            }
            out.push(ContentPart::Text {
                text: s.to_string(),
            });
            continue;
        }
        // Object parts: images, audio, code interpreter payloads. We model the
        // one shape that is stable across exports (asset pointers) and keep
        // everything else whole.
        if let Some(asset) = part.get("asset_pointer").and_then(Value::as_str) {
            out.push(ContentPart::Attachment {
                id: Some(asset.to_string()),
                name: None,
                // ChatGPT's `content_type` here is its own part label
                // ("image_asset_pointer"), not a media type. Putting it in
                // `mime_type` would be a lie, and a consumer would act on it.
                mime_type: None,
                path: None,
                size_bytes: part.get("size_bytes").and_then(Value::as_u64),
            });
            let mut w =
                crate::warning::Warning::new(WarningCode::AttachmentNotIncluded, Severity::Lossy)
                    .for_conversation(conversation_id);
            w.message_id = Some(message_id.to_string());
            warnings.push(w);
            continue;
        }
        warnings.push(
            crate::warning::Warning::new(WarningCode::UnknownContentPart, Severity::Lossy)
                .for_conversation(conversation_id)
                .with_detail(format!("content_type={content_type}")),
        );
        out.push(ContentPart::Unknown {
            kind: Some(content_type.to_string()),
            raw: part.clone(),
        });
    }
    out
}

/// ChatGPT timestamps are float unix seconds.
fn unix_to_rfc3339(v: &Value) -> Option<String> {
    let secs = v.as_f64()?;
    let whole = secs.trunc() as i64;
    let nanos = (secs.fract() * 1_000_000_000.0).round() as u32;
    chrono::DateTime::from_timestamp(whole, nanos).map(|dt| dt.to_rfc3339())
}
