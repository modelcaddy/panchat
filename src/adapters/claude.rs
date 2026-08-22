//! Claude official data export.
//!
//! A multi-file export: `conversations.json`, plus optional `projects.json`,
//! `memories.json`, and `users.json`. Conversations are a flat
//! `chat_messages` array — no branch graph, so no branch data to lose — and
//! carry no per-message model identity, which is itself worth reporting.
//!
//! Newer exports move the side-cars into directories and add a second chat
//! format: `projects/<uuid>.json` one file per project, `design_chats/` for
//! Claude Design's canvas chats, `login_history.json`, and a `memories.json`
//! that is no longer a list of memory rows but one object holding the
//! conversations memory and the memory files. An adapter written for the flat
//! shape reads such an export without complaint and silently returns no
//! projects, no memories, and none of the design chats.

use super::{Adapter, Detection, ExportFile};
use crate::ir::{Artifact, ContentPart, Conversation, Document, Message, ProjectRef, Role, Source};
use crate::warning::{Severity, WarningCode, Warnings};
use crate::Error;
use serde_json::Value;

pub struct Claude;

const PLATFORM: &str = "claude";
/// The flat export: `conversations.json` plus `projects.json` and a
/// `memories.json` of memory rows.
const VARIANT_V1: &str = "official_export_v1";
/// The 2026 export: side-cars split into `projects/` and `design_chats/`
/// directories, and a `memories.json` that is one object rather than rows.
/// Numbered so a consumer can ask "does this export contain design chats?" as
/// `variant_version >= 2` instead of matching strings. See
/// `docs/formats/claude.md`.
const VARIANT_V2: &str = "official_export_v2";

fn find<'a>(files: &'a [ExportFile], leaf: &str) -> Option<&'a ExportFile> {
    files.iter().find(|f| f.lower_path().ends_with(leaf))
}

/// Every `.json` file directly inside a named directory of the export, sorted
/// so output does not depend on the order the filesystem handed them over.
fn in_dir<'a>(files: &'a [ExportFile], dir: &str) -> Vec<&'a ExportFile> {
    let mut out: Vec<&ExportFile> = files
        .iter()
        .filter(|f| {
            let lower = f.lower_path();
            let mut parts: Vec<&str> = lower.split('/').collect();
            let Some(leaf) = parts.pop() else {
                return false;
            };
            leaf.ends_with(".json") && parts.last() == Some(&dir)
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Which generation of the export this is. The directories are the obvious
/// mark; an account with neither projects nor design chats still gets the
/// newer `memories.json`, which is the other.
fn layout(files: &[ExportFile]) -> (&'static str, u32) {
    let v2 = !in_dir(files, "projects").is_empty()
        || !in_dir(files, "design_chats").is_empty()
        || find(files, "memories.json").is_some_and(|f| {
            let text = String::from_utf8_lossy(&f.bytes);
            text.contains("\"conversations_memory\"") || text.contains("\"memory_files\"")
        });
    match v2 {
        true => (VARIANT_V2, 2),
        false => (VARIANT_V1, 1),
    }
}

fn read_object(file: &ExportFile) -> Option<Value> {
    serde_json::from_slice::<Value>(&file.bytes)
        .ok()
        .filter(Value::is_object)
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
        VARIANT_V2
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
        let project_files = in_dir(files, "projects");
        if !project_files.is_empty() {
            notes.push(format!("projects/ ({} file(s))", project_files.len()));
        }
        let design_chats = in_dir(files, "design_chats");
        if !design_chats.is_empty() {
            notes.push(format!("design_chats/ ({} chat(s))", design_chats.len()));
        }
        let (variant, variant_version) = layout(files);
        Some(Detection {
            platform: PLATFORM,
            variant,
            variant_version,
            confidence: 0.97,
            notes,
        })
    }

    fn parse(&self, files: &[ExportFile], warnings: &mut Warnings) -> Result<Document, Error> {
        let file = find_conversations(files)
            .ok_or_else(|| Error::NotRecognized("no conversations.json in export".into()))?;

        let project_files = in_dir(files, "projects");
        let design_chats = in_dir(files, "design_chats");
        let (variant, variant_version) = layout(files);
        let mut doc =
            Document::new(Source::new(PLATFORM, variant).with_variant_version(variant_version));
        for (index, conv) in read_array(file)?.iter().enumerate() {
            match parse_conversation(conv, index, warnings) {
                Some(c) => doc.conversations.push(c),
                None => warnings.note(WarningCode::ItemSkipped, Severity::Dropped),
            }
        }

        // Claude Design's canvas chats are conversations too — a different
        // message shape, in their own directory, and invisible to an adapter
        // that only reads `conversations.json`.
        for f in &design_chats {
            let Some(chat) = read_object(f) else {
                warnings.push(
                    crate::warning::Warning::new(WarningCode::ItemSkipped, Severity::Dropped)
                        .with_detail(format!("{} is not a design chat object", f.path)),
                );
                continue;
            };
            match parse_design_chat(&chat, warnings) {
                Some(c) => doc.conversations.push(c),
                None => warnings.note(WarningCode::ItemSkipped, Severity::Dropped),
            }
        }

        // Side-car files. Projects and memories are not conversations, but
        // dropping them would lose the most useful part of a Claude export.
        if let Some(f) = find(files, "projects.json") {
            for p in read_array(f)? {
                push_project(&mut doc, &p);
            }
        }
        // Newer exports ship one file per project instead.
        for f in &project_files {
            match read_object(f) {
                Some(p) => push_project(&mut doc, &p),
                None => warnings.push(
                    crate::warning::Warning::new(WarningCode::ItemSkipped, Severity::Dropped)
                        .with_detail(format!("{} is not a project object", f.path)),
                ),
            }
        }
        if let Some(f) = find(files, "memories.json") {
            push_memories(&mut doc, f, warnings);
        }
        for leaf in ["users.json", "login_history.json"] {
            if find(files, leaf).is_some() {
                // Account metadata: deliberately not imported, but say so.
                warnings.push(
                    crate::warning::Warning::new(
                        WarningCode::UnhandledExportSection,
                        Severity::Info,
                    )
                    .with_detail(leaf.to_string()),
                );
            }
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

/// A project, plus each document attached to it. The documents are the part a
/// user actually wrote — dropping them keeps the folder and loses its contents.
fn push_project(doc: &mut Document, project: &Value) {
    if let Some(a) = parse_artifact(
        project,
        "project",
        &["name"],
        &["description", "prompt_template"],
    ) {
        doc.artifacts.push(a);
    }
    for item in project
        .get("docs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(a) = parse_artifact(item, "project_doc", &["filename", "title"], &["content"]) {
            doc.artifacts.push(a);
        }
    }
}

/// Memories come in two shapes. Older exports list memory rows, each with its
/// own `uuid`. Newer ones ship a single object: the conversations memory as one
/// block of prose, plus the memory files by path. The second shape has no row
/// ids at all, so parsing it as the first silently yields nothing.
fn push_memories(doc: &mut Document, file: &ExportFile, warnings: &mut Warnings) {
    let Ok(parsed) = serde_json::from_slice::<Value>(&file.bytes) else {
        warnings.push(
            crate::warning::Warning::new(WarningCode::ItemSkipped, Severity::Dropped)
                .with_detail(format!("{} is not readable JSON", file.path)),
        );
        return;
    };
    // The newer shape is wrapped in a one-element array; take either.
    let rows: Vec<&Value> = match &parsed {
        Value::Array(rows) => rows.iter().collect(),
        other => vec![other],
    };

    for row in rows {
        let account = row
            .get("account_uuid")
            .and_then(Value::as_str)
            .unwrap_or("claude");
        let mut modelled = false;

        if let Some(text) = row
            .get("conversations_memory")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            modelled = true;
            doc.artifacts.push(Artifact {
                id: format!("{account}/conversations_memory"),
                kind: "memory".to_string(),
                title: Some("conversations memory".to_string()),
                text: Some(text.to_string()),
                created_at: None,
                raw: None,
            });
        }
        for f in row
            .get("memory_files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(path) = f.get("path").and_then(Value::as_str) else {
                continue;
            };
            modelled = true;
            doc.artifacts.push(Artifact {
                id: format!("{account}{path}"),
                kind: "memory".to_string(),
                title: Some(path.to_string()),
                text: f.get("content").and_then(Value::as_str).map(str::to_string),
                created_at: f.get("created_at").and_then(iso),
                raw: Some(f.clone()),
            });
        }

        if modelled {
            continue;
        }
        // The older row shape, or one we have not seen.
        match parse_artifact(
            row,
            "memory",
            &["title", "name"],
            &["content", "memory", "text"],
        ) {
            Some(a) => doc.artifacts.push(a),
            None => warnings.push(
                crate::warning::Warning::new(
                    WarningCode::UnhandledExportSection,
                    Severity::Dropped,
                )
                .with_detail(format!("unrecognised memory shape in {}", file.path)),
            ),
        }
    }
}

/// A Claude Design canvas chat. Same idea as a conversation, different words
/// for everything: `messages` rather than `chat_messages`, a `content` object
/// rather than a block array, and `contentBlocks` inside it.
fn parse_design_chat(chat: &Value, warnings: &mut Warnings) -> Option<Conversation> {
    let messages_raw = chat.get("messages")?.as_array()?;
    let id = chat.get("uuid").and_then(Value::as_str)?.to_string();

    let mut out = Conversation::new(id.clone());
    out.title = chat
        .get("title")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    out.created_at = chat.get("created_at").and_then(iso);
    out.updated_at = chat.get("updated_at").and_then(iso);
    if let Some(project) = chat.get("project").filter(|v| !v.is_null()) {
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

    let mut previous: Option<String> = None;
    for (i, m) in messages_raw.iter().enumerate() {
        let msg_id = m
            .get("uuid")
            .or_else(|| m.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("{id}-m{i}"));
        let role = m
            .get("role")
            .and_then(Value::as_str)
            .map(Role::parse)
            .unwrap_or_else(|| Role::Other("unknown".into()));

        let mut message = Message::new(msg_id.clone(), role);
        message.parent = previous.clone();
        message.created_at = m.get("created_at").and_then(iso).or_else(|| {
            m.get("content")
                .and_then(|c| c.get("timestamp"))
                .and_then(iso)
        });
        if let Some(content) = m.get("content") {
            message.content = design_content(content, &id, &msg_id, warnings);
        }
        out.messages.push(message);
        previous = Some(msg_id);
    }

    if out.messages.is_empty() {
        return None;
    }
    out.active_path = out.messages.iter().map(|m| m.id.clone()).collect();
    // Which half of the export a conversation came from is not something a
    // consumer can infer, and design chats behave differently enough — no
    // model identity, canvas tool calls, all titled "Chat" — to be worth
    // saying. Namespaced, per the extension rules in SPEC.md.
    out.x.insert(
        "x-panchat".to_string(),
        serde_json::json!({ "claude_export_section": "design_chats" }),
    );
    out.raw = Some(chat.clone());
    Some(out)
}

/// A design chat's `content`: prose in `content`, typed blocks in
/// `contentBlocks`, and attachments whose bytes never left the browser but
/// whose text is inline.
fn design_content(
    content: &Value,
    conversation_id: &str,
    message_id: &str,
    warnings: &mut Warnings,
) -> Vec<ContentPart> {
    if let Some(text) = content.as_str() {
        return match text.is_empty() {
            true => Vec::new(),
            false => vec![ContentPart::Text {
                text: text.to_string(),
            }],
        };
    }

    let mut out = Vec::new();
    if let Some(text) = content
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        out.push(ContentPart::Text {
            text: text.to_string(),
        });
    }

    for block in content
        .get("contentBlocks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let kind = block.get("type").and_then(Value::as_str).unwrap_or("text");
        match kind {
            "text" => {
                if let Some(t) = block
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    out.push(ContentPart::Text {
                        text: t.to_string(),
                    });
                }
            }
            "tool_call" => {
                let call = block.get("toolCall").unwrap_or(block);
                let name = call.get("name").and_then(Value::as_str).map(str::to_string);
                out.push(ContentPart::ToolUse {
                    name: name.clone(),
                    input: call.get("input").cloned(),
                });
                if let Some(output) = call.get("output").filter(|v| !v.is_null()) {
                    out.push(ContentPart::ToolResult {
                        name,
                        output: Some(output.clone()),
                    });
                }
            }
            // An empty thinking block is the vendor's placeholder, not a
            // thought that was lost.
            "thinking"
                if block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .is_empty() => {}
            other => {
                warnings.push(
                    crate::warning::Warning::new(WarningCode::UnknownContentPart, Severity::Lossy)
                        .for_conversation(conversation_id)
                        .with_detail(format!("design block type={other}")),
                );
                out.push(ContentPart::Unknown {
                    kind: Some(other.to_string()),
                    raw: block.clone(),
                });
            }
        }
    }

    for att in content
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.push(ContentPart::Attachment {
            id: att.get("id").and_then(Value::as_str).map(str::to_string),
            name: att.get("name").and_then(Value::as_str).map(str::to_string),
            // `type` here is a design-side label — "comment", "file" — not a
            // media type, and a consumer would act on it if we put it there.
            mime_type: None,
            path: None,
            size_bytes: None,
        });
        // Design attachments carry their text inline; that text was part of
        // the prompt, so it belongs in the transcript rather than in a warning
        // about bytes we never needed.
        match att
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            Some(text) => out.push(ContentPart::Text {
                text: text.to_string(),
            }),
            None => {
                let mut w = crate::warning::Warning::new(
                    WarningCode::AttachmentNotIncluded,
                    Severity::Lossy,
                )
                .for_conversation(conversation_id);
                w.message_id = Some(message_id.to_string());
                warnings.push(w);
            }
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
