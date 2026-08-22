//! ChatGPT official data export (`conversations.json`, or `conversations-000.json`
//! … `conversations-NNN.json` once the account is large enough to be sharded).
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
//!
//! The second way to lose data here is quieter. Large exports no longer ship
//! one `conversations.json`; they ship a hundred conversations per file plus an
//! `export_manifest.json` naming the shards, a
//! `conversation_asset_file_names.json` mapping asset ids to the names the user
//! uploaded them under, and the attachment bytes themselves as `file-*.dat`.
//! An importer that reads the first file it recognises keeps 100 conversations
//! out of 1,285 and reports no error at all.

use super::{Adapter, Detection, ExportFile};
use crate::ir::{ContentPart, Conversation, Document, Message, ProjectRef, Role, Source};
use crate::warning::{Severity, Warning, WarningCode, Warnings};
use crate::Error;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub struct ChatGpt;

const PLATFORM: &str = "chatgpt";
/// The single-file export: one `conversations.json`, attachment bytes rarely
/// included and named after the asset id when they are.
const VARIANT_V1: &str = "official_export_v1";
/// The 2026 export: a manifest, conversations split across numbered shards, an
/// asset-name map, and the attachment bytes alongside. Numbered rather than
/// merely named so a consumer can ask "does this export ship its attachments?"
/// as `variant_version >= 2` instead of matching strings. See
/// `docs/formats/chatgpt.md`.
const VARIANT_V2: &str = "official_export_v2";
const MANIFEST: &str = "export_manifest.json";
const ASSET_NAMES: &str = "conversation_asset_file_names.json";

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The id inside an asset pointer: `file-service://file-abc` → `file-abc`.
/// Two schemes are in use — `file-service` for uploads and generated images,
/// `sediment` for voice and video — and neither changes what the id means.
fn asset_id(pointer: &str) -> &str {
    pointer.rsplit("://").next().unwrap_or(pointer)
}

fn file_named<'a>(files: &'a [ExportFile], name: &str) -> Option<&'a ExportFile> {
    files.iter().find(|f| {
        let lower = f.lower_path();
        basename(&lower) == name
    })
}

/// Every file of conversations, in the order they belong in.
///
/// The manifest is the export's own statement of which files hold
/// conversations, so it is trusted first. Filename pattern is the fallback,
/// and shape is the fallback to that — a user who renamed their export, or
/// unpacked it oddly, must not be told their data is unrecognisable. Detection
/// must never depend on a filename we do not control.
fn conversation_shards(files: &[ExportFile]) -> Vec<&ExportFile> {
    if let Some((shards, _)) = shards_from_manifest(files) {
        if !shards.is_empty() {
            return shards;
        }
    }

    let mut named: Vec<&ExportFile> = files
        .iter()
        .filter(|f| {
            let lower = f.lower_path();
            let base = basename(&lower);
            base.starts_with("conversations") && base.ends_with(".json")
        })
        .collect();
    named.sort_by(|a, b| a.path.cmp(&b.path));
    // Claude's export also has a `conversations.json`; the shape test is what
    // keeps this adapter from claiming it.
    if named.iter().any(|f| looks_like_chatgpt(&f.bytes)) {
        return named;
    }

    let mut sniffed: Vec<&ExportFile> = files
        .iter()
        .filter(|f| f.lower_path().ends_with(".json") && looks_like_chatgpt(&f.bytes))
        .collect();
    sniffed.sort_by(|a, b| a.path.cmp(&b.path));
    sniffed
}

/// Which generation of the export this is.
///
/// Sharding alone is the wrong test: a small 2026 account gets one
/// `conversations.json` in the new layout. The manifest and the asset-name map
/// are what actually mark it.
fn layout(files: &[ExportFile], shards: usize) -> (&'static str, u32) {
    let v2 = shards > 1
        || file_named(files, MANIFEST).is_some()
        || file_named(files, ASSET_NAMES).is_some();
    match v2 {
        true => (VARIANT_V2, 2),
        false => (VARIANT_V1, 1),
    }
}

/// Shards named by `export_manifest.json`, plus the names it listed that are
/// not in the export — a truncated download is worth saying out loud rather
/// than reporting a smaller conversation count as if it were the whole thing.
fn shards_from_manifest(files: &[ExportFile]) -> Option<(Vec<&ExportFile>, Vec<String>)> {
    let manifest = file_named(files, MANIFEST)?;
    let parsed: Value = serde_json::from_slice(&manifest.bytes).ok()?;
    let listed = parsed
        .get("logical_files")?
        .get("conversations.json")?
        .get("files")?
        .as_array()?;

    let mut found = Vec::new();
    let mut missing = Vec::new();
    for name in listed.iter().filter_map(Value::as_str) {
        let lower = name.to_ascii_lowercase();
        match file_named(files, basename(&lower)) {
            Some(f) => found.push(f),
            None => missing.push(name.to_string()),
        }
    }
    Some((found, missing))
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

/// The non-conversation files shipped alongside the chats: the asset name map,
/// and the attachment bytes themselves.
struct Assets<'a> {
    /// `file-abc.dat` (lowercased) → the name the user uploaded it under.
    names: BTreeMap<String, String>,
    /// Lowercased basename → path relative to the export root.
    paths: HashMap<String, &'a str>,
    /// The same files as `(lowercased basename, path)`, for the older layout
    /// where the bytes keep their original name: `file-abc-photo.png`, or
    /// `dalle-generations/file-abc.webp`.
    by_prefix: Vec<(String, &'a str)>,
}

/// What an export could tell us about one referenced file.
struct Resolved {
    path: Option<String>,
    name: Option<String>,
}

impl<'a> Assets<'a> {
    fn index(files: &'a [ExportFile]) -> Self {
        let mut names = BTreeMap::new();
        if let Some(f) = file_named(files, ASSET_NAMES) {
            if let Ok(Value::Object(map)) = serde_json::from_slice::<Value>(&f.bytes) {
                for (key, value) in map {
                    if let Some(name) = value.as_str() {
                        names.insert(key.to_ascii_lowercase(), name.to_string());
                    }
                }
            }
        }
        let by_prefix: Vec<(String, &str)> = files
            .iter()
            .map(|f| (basename(&f.lower_path()).to_string(), f.path.as_str()))
            .collect();
        let paths = by_prefix.iter().cloned().collect();
        Self {
            names,
            paths,
            by_prefix,
        }
    }

    /// Where an asset pointer's bytes are, if they shipped.
    ///
    /// Current exports store each attachment as `<asset id>.dat` and put the
    /// original filename in `conversation_asset_file_names.json`. Older ones
    /// kept the name on the file — `file-abc-diagram.png`, and DALL·E images
    /// under `dalle-generations/` — with no map at all, so the id is a prefix
    /// there rather than the whole name.
    fn resolve(&self, pointer: &str) -> Resolved {
        let id = asset_id(pointer).to_ascii_lowercase();
        let key = format!("{id}.dat");
        if let Some(path) = self.paths.get(&key) {
            return Resolved {
                path: Some(path.to_string()),
                name: self.names.get(&key).cloned(),
            };
        }
        let found = self.by_prefix.iter().find(|(base, _)| {
            base.starts_with(&id) && base[id.len()..].starts_with(['.', '-', '_'])
        });
        Resolved {
            path: found.map(|(_, path)| path.to_string()),
            // The old layout carries the name on the file itself.
            name: found.map(|(base, _)| base.clone()),
        }
    }
}

impl Adapter for ChatGpt {
    fn platform(&self) -> &'static str {
        PLATFORM
    }

    fn variant(&self) -> &'static str {
        VARIANT_V2
    }

    fn detect(&self, files: &[ExportFile]) -> Option<Detection> {
        let shards = conversation_shards(files);
        let first = shards.first()?;
        let parsed: Value = serde_json::from_slice(&first.bytes).ok()?;
        let array = parsed.as_array()?;
        let head = array.first()?;
        // Export rows are identifiable by their shape: a `mapping` node graph
        // plus a `current_node` pointer.
        let has_mapping = head.get("mapping").map(Value::is_object).unwrap_or(false);
        let has_current_node = head.get("current_node").is_some();
        let has_title = head.get("title").is_some();
        let confidence = match (has_mapping, has_current_node, has_title) {
            (true, true, _) => 0.98,
            (true, false, true) => 0.85,
            (false, _, true) => 0.40,
            _ => return None,
        };

        let (variant, variant_version) = layout(files, shards.len());
        let sharded = shards.len() > 1;
        let mut notes = Vec::new();
        if sharded {
            // Counting every conversation would mean parsing every shard, which
            // is the expensive half of the work `detect` exists to avoid.
            notes.push(format!("{} conversation file(s)", shards.len()));
            notes.push(format!("{} conversation(s) in {}", array.len(), first.path));
        } else {
            notes.push(format!("{} conversation(s)", array.len()));
        }
        Some(Detection {
            platform: PLATFORM,
            variant,
            variant_version,
            confidence,
            notes,
        })
    }

    fn parse(&self, files: &[ExportFile], warnings: &mut Warnings) -> Result<Document, Error> {
        let shards = conversation_shards(files);
        if shards.is_empty() {
            return Err(Error::NotRecognized(
                "no conversations.json in export".into(),
            ));
        }
        let assets = Assets::index(files);

        let (variant, variant_version) = layout(files, shards.len());
        let mut doc =
            Document::new(Source::new(PLATFORM, variant).with_variant_version(variant_version));

        // A shard the manifest promised but the download does not contain is a
        // hole in the data, not a smaller export.
        if let Some((_, missing)) = shards_from_manifest(files) {
            for name in missing {
                warnings.push(
                    Warning::new(WarningCode::UnhandledExportSection, Severity::Dropped)
                        .with_detail(format!("{name} listed in {MANIFEST} but not in the export")),
                );
            }
        }

        // Sections we knowingly do not import. Old exports carry feedback and
        // model comparisons; new ones add `ads.json`. Both carry `chat.html`,
        // which is the same conversations rendered for a browser.
        for leaf in [
            "user.json",
            "message_feedback.json",
            "model_comparisons.json",
            "shared_conversations.json",
            "ads.json",
            "chat.html",
        ] {
            if file_named(files, leaf).is_some() {
                warnings.push(
                    Warning::new(WarningCode::UnhandledExportSection, Severity::Info)
                        .with_detail(leaf.to_string()),
                );
            }
        }

        let mut index = 0usize;
        for shard in &shards {
            let parsed: Value = serde_json::from_slice(&shard.bytes)?;
            let Some(array) = parsed.as_array() else {
                // One unreadable shard must not cost the user the others.
                warnings.push(
                    Warning::new(WarningCode::UnhandledExportSection, Severity::Dropped)
                        .with_detail(format!("{} is not a JSON array", shard.path)),
                );
                continue;
            };
            for conv in array {
                match parse_conversation(conv, index, &assets, warnings) {
                    Some(c) => doc.conversations.push(c),
                    None => warnings.note(WarningCode::ItemSkipped, Severity::Dropped),
                }
                index += 1;
            }
        }
        Ok(doc)
    }
}

fn parse_conversation(
    conv: &Value,
    index: usize,
    assets: &Assets,
    warnings: &mut Warnings,
) -> Option<Conversation> {
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
        message.content = extract_content(msg, assets, &id, node_id, warnings);
        append_metadata_attachments(&mut message, msg, assets, &id, node_id, warnings);
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
    assets: &Assets,
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
        // Reasoning turns (`thoughts`, `reasoning_recap`) land here. They are
        // kept whole rather than folded into the answer's text: a summary of
        // the model's thinking is not what the assistant said, and a consumer
        // that concatenated the two would put words in its mouth.
        warnings.push(
            Warning::new(WarningCode::UnknownContentPart, Severity::Lossy)
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
        // shapes that are stable across exports (asset pointers, voice
        // transcripts) and keep everything else whole.
        if let Some(pointer) = part.get("asset_pointer").and_then(Value::as_str) {
            out.push(attachment_part(
                assets,
                pointer,
                None,
                None,
                part.get("size_bytes").and_then(Value::as_u64),
                conversation_id,
                message_id,
                warnings,
            ));
            continue;
        }
        let part_type = part.get("content_type").and_then(Value::as_str);
        // A voice turn's transcript is the turn. Left as `unknown` it reads as
        // an empty message with an audio blob attached.
        if part_type == Some("audio_transcription") {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    out.push(ContentPart::Text {
                        text: text.to_string(),
                    });
                }
                continue;
            }
        }
        // Realtime voice and video parts carry no `asset_pointer` of their own;
        // theirs hang one level down.
        let nested = nested_pointers(part);
        if !nested.is_empty() {
            for (pointer, size_bytes) in nested {
                out.push(attachment_part(
                    assets,
                    &pointer,
                    None,
                    None,
                    size_bytes,
                    conversation_id,
                    message_id,
                    warnings,
                ));
            }
            continue;
        }
        warnings.push(
            Warning::new(WarningCode::UnknownContentPart, Severity::Lossy)
                .for_conversation(conversation_id)
                .with_detail(format!(
                    "content_type={}",
                    part_type.unwrap_or(content_type)
                )),
        );
        out.push(ContentPart::Unknown {
            kind: Some(part_type.unwrap_or(content_type).to_string()),
            raw: part.clone(),
        });
    }
    out
}

/// Asset pointers nested inside a realtime voice or video part.
fn nested_pointers(part: &Value) -> Vec<(String, Option<u64>)> {
    fn push(value: &Value, out: &mut Vec<(String, Option<u64>)>) {
        if let Some(pointer) = value.get("asset_pointer").and_then(Value::as_str) {
            out.push((
                pointer.to_string(),
                value.get("size_bytes").and_then(Value::as_u64),
            ));
        }
    }

    let mut out = Vec::new();
    for key in ["audio_asset_pointer", "video_container_asset_pointer"] {
        if let Some(value) = part.get(key) {
            push(value, &mut out);
        }
    }
    if let Some(frames) = part.get("frames_asset_pointers").and_then(Value::as_array) {
        for frame in frames {
            push(frame, &mut out);
        }
    }
    out
}

/// Files the user uploaded are recorded in `metadata.attachments`, not in the
/// content parts — the parts hold only the text of the turn. Without this, a
/// turn that was "here is the CSV" plus a 6 MB CSV that shipped in the export
/// looks like a conversation about nothing.
fn append_metadata_attachments(
    message: &mut Message,
    msg: &Value,
    assets: &Assets,
    conversation_id: &str,
    message_id: &str,
    warnings: &mut Warnings,
) {
    let listed = msg
        .get("metadata")
        .and_then(|m| m.get("attachments"))
        .and_then(Value::as_array);
    let Some(listed) = listed else {
        return;
    };

    // An image upload appears both here and as an `image_asset_pointer` part.
    let already: HashSet<String> = message
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Attachment { id: Some(id), .. } => Some(asset_id(id).to_string()),
            _ => None,
        })
        .collect();

    for item in listed {
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        if already.contains(asset_id(id)) {
            continue;
        }
        message.content.push(attachment_part(
            assets,
            id,
            item.get("name").and_then(Value::as_str).map(str::to_string),
            // The vendor records a real media type here, unlike the part-level
            // `content_type`, so it can be passed through as one.
            item.get("mime_type")
                .and_then(Value::as_str)
                .map(str::to_string),
            item.get("size")
                .or_else(|| item.get("size_bytes"))
                .and_then(Value::as_u64),
            conversation_id,
            message_id,
            warnings,
        ));
    }
}

/// One referenced file, resolved against the bytes the export shipped.
///
/// Whether the bytes are present is the whole question for a consumer, so it is
/// answered on the part (`path`) and, when they are absent, in a warning.
#[allow(clippy::too_many_arguments)]
fn attachment_part(
    assets: &Assets,
    pointer: &str,
    name: Option<String>,
    mime_type: Option<String>,
    size_bytes: Option<u64>,
    conversation_id: &str,
    message_id: &str,
    warnings: &mut Warnings,
) -> ContentPart {
    let found = assets.resolve(pointer);
    if found.path.is_none() {
        let mut w = Warning::new(WarningCode::AttachmentNotIncluded, Severity::Lossy)
            .for_conversation(conversation_id);
        w.message_id = Some(message_id.to_string());
        warnings.push(w);
    }
    ContentPart::Attachment {
        id: Some(pointer.to_string()),
        name: name.or(found.name),
        mime_type,
        path: found.path,
        size_bytes,
    }
}

/// ChatGPT timestamps are float unix seconds.
fn unix_to_rfc3339(v: &Value) -> Option<String> {
    let secs = v.as_f64()?;
    let whole = secs.trunc() as i64;
    let nanos = (secs.fract() * 1_000_000_000.0).round() as u32;
    chrono::DateTime::from_timestamp(whole, nanos).map(|dt| dt.to_rfc3339())
}
