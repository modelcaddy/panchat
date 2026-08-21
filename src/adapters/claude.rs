//! Claude official data export.
//!
//! A multi-file export: `conversations.json`, plus optional `projects.json`,
//! `memories.json`, and `users.json`. Conversations are a flat
//! `chat_messages` array — no branch graph, so no branch data to lose — and
//! carry no per-message model identity, which is itself worth reporting.

use super::{Adapter, Detection, ExportFile};
use crate::ir::{Artifact, ContentPart, Conversation, Document, Message, ProjectRef, Role, Source};
use crate::warning::{Severity, WarningCode, Warnings};
use crate::Error;
use serde_json::Value;

pub struct Claude;

const PLATFORM: &str = "claude";
const VARIANT: &str = "official_export_v1";

fn find<'a>(files: &'a [ExportFile], leaf: &str) -> Option<&'a ExportFile> {
    files.iter().find(|f| f.lower_path().ends_with(leaf))
}

/// Locate the conversations file by name, falling back to shape. A renamed or
/// oddly-unpacked export must still be readable — detection must never depend
/// on a filename we do not control.
fn find_conversations(files: &[ExportFile]) -> Option<&ExportFile> {
    if let Some(f) = find(files, "conversations.json") {
        return Some(f);
    }
    files
        .iter()
        .find(|f| f.lower_path().ends_with(".json") && looks_like_claude(&f.bytes))
}

/// Shape test: a JSON array whose first element has `uuid` and `chat_messages`.
fn looks_like_claude(bytes: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let Some(first) = v.as_array().and_then(|a| a.first()) else {
        return false;
    };
    first.get("uuid").is_some()
        && first
            .get("chat_messages")
            .map(Value::is_array)
            .unwrap_or(false)
}

fn read_array(file: &ExportFile) -> Result<Vec<Value>, Error> {
    let v: Value = serde_json::from_slice(&file.bytes)?;
    v.as_array()
        .cloned()
        .ok_or_else(|| Error::Malformed(format!("{} must be a JSON array", file.path)))
}

impl Adapter for Claude {
    fn platform(&self) -> &'static str {
        PLATFORM
    }

    fn variant(&self) -> &'static str {
        VARIANT
    }

    fn detect(&self, files: &[ExportFile]) -> Option<Detection> {
        let file = find_conversations(files)?;
        let parsed: Value = serde_json::from_slice(&file.bytes).ok()?;
        let array = parsed.as_array()?;
        let first = array.first()?;
        // Claude rows carry `uuid` + `chat_messages`; ChatGPT rows carry
        // `mapping`. The two are unambiguous.
        let has_uuid = first.get("uuid").is_some();
        let has_messages = first
            .get("chat_messages")
            .map(Value::is_array)
            .unwrap_or(false);
        if !(has_uuid && has_messages) {
            return None;
        }
        let mut notes = vec![format!("{} conversation(s)", array.len())];
        if find(files, "projects.json").is_some() {
            notes.push("projects.json present".into());
        }
        if find(files, "memories.json").is_some() {
            notes.push("memories.json present".into());
        }
        Some(Detection {
            platform: PLATFORM,
            variant: VARIANT,
            confidence: 0.97,
            notes,
        })
    }

    fn parse(&self, files: &[ExportFile], warnings: &mut Warnings) -> Result<Document, Error> {
        let file = find_conversations(files)
            .ok_or_else(|| Error::NotRecognized("no conversations.json in export".into()))?;

        let mut doc = Document::new(Source::new(PLATFORM, VARIANT));
        for (index, conv) in read_array(file)?.iter().enumerate() {
            match parse_conversation(conv, index, warnings) {
                Some(c) => doc.conversations.push(c),
                None => warnings.note(WarningCode::ItemSkipped, Severity::Dropped),
            }
        }

        // Side-car files. Projects and memories are not conversations, but
        // dropping them would lose the most useful part of a Claude export.
        if let Some(f) = find(files, "projects.json") {
            for p in read_array(f)? {
                if let Some(a) = parse_artifact(
                    &p,
                    "project",
                    &["name"],
                    &["description", "prompt_template"],
                ) {
                    doc.artifacts.push(a);
                }
            }
        }
        if let Some(f) = find(files, "memories.json") {
            for m in read_array(f)? {
                if let Some(a) = parse_artifact(
                    &m,
                    "memory",
                    &["title", "name"],
                    &["content", "memory", "text"],
                ) {
                    doc.artifacts.push(a);
                }
            }
        }
        if find(files, "users.json").is_some() {
            // Account metadata: deliberately not imported, but say so.
            warnings.note(WarningCode::UnhandledExportSection, Severity::Info);
        }

        // Format-level, not item-level: no Claude export records this.
        warnings.note(WarningCode::NoModelIdentity, Severity::Info);
        Ok(doc)
    }
}

fn parse_conversation(conv: &Value, index: usize, warnings: &mut Warnings) -> Option<Conversation> {
    let messages_raw = conv.get("chat_messages")?.as_array()?;

    let id = conv
        .get("uuid")
        .or_else(|| conv.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            warnings.note(WarningCode::SynthesizedId, Severity::Info);
            format!("claude-conversation-{index}")
        });

    let mut out = Conversation::new(id.clone());
    out.title = conv
        .get("name")
        .or_else(|| conv.get("title"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    out.created_at = conv.get("created_at").and_then(iso);
    out.updated_at = conv.get("updated_at").and_then(iso);
    if out.created_at.is_none() && out.updated_at.is_none() {
        warnings.note_for(WarningCode::MissingTimestamps, Severity::Info, &id);
    }
    if let Some(project) = conv.get("project").filter(|v| !v.is_null()) {
        if let Some(pid) = project.get("uuid").and_then(Value::as_str) {
            out.project = Some(ProjectRef {
                id: pid.to_string(),
                name: project
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
        }
    }

    // Flat list: parent is simply the previous message. Recording the edges
    // anyway keeps the IR uniform across vendors.
    let mut previous: Option<String> = None;
    for (i, m) in messages_raw.iter().enumerate() {
        let msg_id = m
            .get("uuid")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{id}-m{i}"));
        let role = m
            .get("sender")
            .or_else(|| m.get("role"))
            .and_then(Value::as_str)
            .map(Role::parse)
            .unwrap_or_else(|| Role::Other("unknown".into()));

        let mut message = Message::new(msg_id.clone(), role);
        message.parent = previous.clone();
        message.created_at = m.get("created_at").and_then(iso);
        message.content = extract_content(m, &id, &msg_id, warnings);
        out.messages.push(message);
        previous = Some(msg_id);
    }

    if out.messages.is_empty() {
        return None;
    }
    // A flat export has exactly one branch, and it is the active one.
    out.active_path = out.messages.iter().map(|m| m.id.clone()).collect();
    out.raw = Some(conv.clone());
    Some(out)
}

/// Claude messages carry `text` and/or a `content` array of typed blocks, plus
/// a separate `attachments`/`files` list.
fn extract_content(
    m: &Value,
    conversation_id: &str,
    message_id: &str,
    warnings: &mut Warnings,
) -> Vec<ContentPart> {
    let mut out = Vec::new();

    if let Some(blocks) = m.get("content").and_then(Value::as_array) {
        for block in blocks {
            let kind = block.get("type").and_then(Value::as_str).unwrap_or("text");
            match kind {
                "text" => {
                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                        if !t.is_empty() {
                            out.push(ContentPart::Text {
                                text: t.to_string(),
                            });
                        }
                    }
                }
                "tool_use" => out.push(ContentPart::ToolUse {
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    input: block.get("input").cloned(),
                }),
                "tool_result" => out.push(ContentPart::ToolResult {
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    output: block.get("content").cloned(),
                }),
                other => {
                    warnings.push(
                        crate::warning::Warning::new(
                            WarningCode::UnknownContentPart,
                            Severity::Lossy,
                        )
                        .for_conversation(conversation_id)
                        .with_detail(format!("block type={other}")),
                    );
                    out.push(ContentPart::Unknown {
                        kind: Some(other.to_string()),
                        raw: block.clone(),
                    });
                }
            }
        }
    }

    // `text` duplicates the text blocks when both are present.
    if out.is_empty() {
        if let Some(t) = m.get("text").and_then(Value::as_str) {
            if !t.is_empty() {
                out.push(ContentPart::Text {
                    text: t.to_string(),
                });
            }
        }
    }

    for key in ["attachments", "files"] {
        let Some(list) = m.get(key).and_then(Value::as_array) else {
            continue;
        };
        for att in list {
            out.push(ContentPart::Attachment {
                id: att
                    .get("id")
                    .or_else(|| att.get("uuid"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                name: att
                    .get("file_name")
                    .or_else(|| att.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                mime_type: att
                    .get("file_type")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                path: None,
                size_bytes: att.get("file_size").and_then(Value::as_u64),
            });
            let mut w =
                crate::warning::Warning::new(WarningCode::AttachmentNotIncluded, Severity::Lossy)
                    .for_conversation(conversation_id);
            w.message_id = Some(message_id.to_string());
            warnings.push(w);
        }
    }

    out
}

fn parse_artifact(
    v: &Value,
    kind: &str,
    title_keys: &[&str],
    text_keys: &[&str],
) -> Option<Artifact> {
    let id = v
        .get("uuid")
        .or_else(|| v.get("id"))
        .and_then(Value::as_str)?
        .to_string();
    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|k| v.get(*k).and_then(Value::as_str))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(Artifact {
        id,
        kind: kind.to_string(),
        title: pick(title_keys),
        text: pick(text_keys),
        created_at: v.get("created_at").and_then(iso),
        raw: Some(v.clone()),
    })
}

/// Claude timestamps are already ISO 8601; normalize to RFC 3339 so every
/// adapter emits one shape.
fn iso(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    match chrono::DateTime::parse_from_rfc3339(s) {
        Ok(dt) => Some(dt.to_rfc3339()),
        // Keep an unparseable-but-present timestamp rather than dropping it.
        Err(_) => Some(s.to_string()),
    }
}
