//! Claude Code's local session history (`~/.claude/projects/`).
//!
//! The first producer in this crate that reads a *client's own history* rather
//! than a vendor's export, and the richest source here: every session is a JSONL
//! file of nodes carrying `uuid` and `parentUuid`, so the branch graph is real —
//! every rewind, every edited prompt, every retry — and every assistant message
//! names the model that wrote it. What ChatGPT's export has and Claude's lacks,
//! this has both of.
//!
//! **Observed, not reconstructed.** The shape below was read, as key names and
//! counts only, from 171 real sessions and 139 subagent transcripts on the
//! machine this was written on. `docs/formats/claude-code.md` records what was
//! seen and the numbers.
//!
//! What the adapter has to get right:
//!
//! - **The chain includes nodes that are not messages.** Attachments (hook
//!   output, files) and system events carry a `uuid` and sit in the parent
//!   chain. Skipping them would leave every message after one pointing at a
//!   parent that is not there, so they are kept, as hidden messages, with the
//!   vendor's payload intact.
//! - **Compaction starts a new root.** After a context compaction the next node
//!   has no `parentUuid` and a `logicalParentUuid` naming where it came from.
//!   That is the vendor's own link, and following it joins every observed
//!   session into one tree.
//! - **A node can be written more than once.** Resuming a session rewrites
//!   earlier nodes with new session metadata — 12,867 of them in the sample.
//!   The last write wins, and the rare rewrite that changed a message rather than
//!   its metadata is reported.
//! - **The active branch is named.** A `last-prompt` line carries a `leafUuid`;
//!   walking up from it is the thread the user last saw.

use super::{Adapter, Detection, ExportFile};
use crate::ir::{Artifact, ContentPart, Conversation, Document, Message, ProjectRef, Role, Source};
use crate::warning::{Severity, Warning, WarningCode, Warnings};
use crate::Error;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

pub struct ClaudeCode;

const PLATFORM: &str = "claude-code";
/// The JSONL session store as of 2026. Numbered from 1; the number is ours.
const VARIANT_V1: &str = "local_history_v1";

/// Line types that are nodes in the conversation graph.
const NODE_TYPES: [&str; 4] = ["user", "assistant", "attachment", "system"];

/// Envelope fields on a message line that the representation does not hold.
/// Named in the warning that says so, rather than dropped quietly.
const UNMODELLED: &str =
    "usage, stop_reason, requestId, toolUseResult, and per-line cwd/gitBranch/version";

impl Adapter for ClaudeCode {
    fn platform(&self) -> &str {
        PLATFORM
    }

    fn variant(&self) -> &str {
        VARIANT_V1
    }

    fn detect(&self, files: &[ExportFile]) -> Option<Detection> {
        let sessions = files.iter().filter(|f| is_session(f)).count();
        if sessions == 0 {
            return None;
        }
        Some(Detection {
            platform: PLATFORM.into(),
            variant: VARIANT_V1.into(),
            variant_version: 1,
            confidence: 0.95,
            notes: vec![format!("{sessions} session file(s)")],
        })
    }

    fn parse(&self, files: &[ExportFile], warnings: &mut Warnings) -> Result<Document, Error> {
        let mut doc = Document::new(Source::capture(PLATFORM, VARIANT_V1).with_variant_version(1));

        let mut sessions: Vec<&ExportFile> = files.iter().filter(|f| is_session(f)).collect();
        sessions.sort_by(|a, b| a.path.cmp(&b.path));
        if sessions.is_empty() {
            return Err(Error::Malformed("no Claude Code session files".into()));
        }

        let mut tally = Tally::default();
        for file in &sessions {
            if let Some(c) = session(file, warnings, &mut tally) {
                doc.conversations.push(c);
            }
        }

        // Memory files Claude Code keeps per project. Small, written by the
        // user or by the tool on their behalf, and exactly the side-car a
        // consumer building memory wants.
        for f in files.iter().filter(|f| is_memory_file(f)) {
            doc.artifacts.push(Artifact {
                id: f.path.clone(),
                kind: "memory".into(),
                title: f.path.rsplit('/').next().map(str::to_string),
                text: Some(String::from_utf8_lossy(&f.bytes).into_owned()),
                created_at: None,
                raw: None,
            });
        }

        tally.report(files, warnings);
        Ok(doc)
    }
}

/// Counts folded into one warning each, because a session store holds tens of
/// thousands of thinking blocks and ten thousand identical lines would bury
/// everything else the document has to say.
#[derive(Default)]
struct Tally {
    unknown_parts: BTreeMap<String, u32>,
    rewrites_changed_content: u32,
    envelope_lines: u32,
}

impl Tally {
    fn report(self, files: &[ExportFile], warnings: &mut Warnings) {
        for (kind, n) in self.unknown_parts {
            let mut w = Warning::new(WarningCode::UnknownContentPart, Severity::Lossy)
                .with_detail(format!("block type={kind}, kept verbatim"));
            w.count = n;
            warnings.push(w);
        }
        if self.rewrites_changed_content > 0 {
            let mut w = Warning::new(WarningCode::UnhandledExportSection, Severity::Lossy)
                .with_detail(
                    "a session rewrote an earlier node with a different message or parent; the \
                     last write is represented and the earlier one is not",
                );
            w.count = self.rewrites_changed_content;
            warnings.push(w);
        }
        if self.envelope_lines > 0 {
            let mut w = Warning::new(WarningCode::UnhandledExportSection, Severity::Lossy)
                .with_detail(format!(
                    "fields on each message line are not represented: {UNMODELLED}"
                ));
            w.count = self.envelope_lines;
            warnings.push(w);
        }
        // Tool output too large for the transcript is written beside it and
        // referenced by path. It is real content and it is not read.
        let spilled = files
            .iter()
            .filter(|f| f.path.contains("/tool-results/"))
            .count();
        if spilled > 0 {
            let mut w = Warning::new(WarningCode::UnhandledExportSection, Severity::Lossy)
                .with_detail(
                    "tool output saved beside the transcript in tool-results/ is not read",
                );
            w.count = spilled as u32;
            warnings.push(w);
        }
        let other_jsonl = files
            .iter()
            .filter(|f| f.lower_path().ends_with(".jsonl") && f.loaded && !is_session(f))
            .count();
        if other_jsonl > 0 {
            let mut w = Warning::new(WarningCode::UnhandledExportSection, Severity::Info)
                .with_detail("JSONL files that are not sessions (workflow journals and the like)");
            w.count = other_jsonl as u32;
            warnings.push(w);
        }
    }
}

/// A session file, recognised by its lines rather than its name: a JSONL line
/// that is a user or assistant node carrying a `sessionId` and a `message`.
///
/// Only the first lines are looked at. `detect` runs on every input, and a
/// session can be 150 MB.
fn is_session(f: &ExportFile) -> bool {
    if !f.loaded || !f.lower_path().ends_with(".jsonl") {
        return false;
    }
    f.bytes
        .split(|b| *b == b'\n')
        .take(50)
        .filter_map(|line| serde_json::from_slice::<Value>(line).ok())
        .any(|v| {
            matches!(
                v.get("type").and_then(Value::as_str),
                Some("user" | "assistant")
            ) && v.get("sessionId").is_some_and(Value::is_string)
                && v.get("message").is_some_and(Value::is_object)
                && v.get("uuid").is_some_and(Value::is_string)
        })
}

fn is_memory_file(f: &ExportFile) -> bool {
    f.loaded && f.lower_path().ends_with(".md") && f.path.contains("/memory/")
}

/// One node of the graph, after the last write for its uuid has won.
struct Node {
    line: Value,
}

fn session(file: &ExportFile, warnings: &mut Warnings, tally: &mut Tally) -> Option<Conversation> {
    // Node order is first appearance; content is last write.
    let mut order: Vec<String> = Vec::new();
    let mut nodes: HashMap<String, Node> = HashMap::new();
    let mut other_lines: Vec<Value> = Vec::new();
    let mut leaf: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut ai_title: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut unreadable = 0u32;

    for raw_line in file.bytes.split(|b| *b == b'\n') {
        if raw_line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(line) = serde_json::from_slice::<Value>(raw_line) else {
            unreadable += 1;
            continue;
        };
        let kind = line.get("type").and_then(Value::as_str).unwrap_or_default();
        if session_id.is_none() {
            session_id = line
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        match kind {
            "last-prompt" => {
                leaf = line
                    .get("leafUuid")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                other_lines.push(line);
            }
            // Renamed sessions write a new title line; the last one is current.
            "custom-title" => {
                custom_title = line
                    .get("customTitle")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                other_lines.push(line);
            }
            "ai-title" => {
                ai_title = line
                    .get("aiTitle")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                other_lines.push(line);
            }
            k if NODE_TYPES.contains(&k) && line.get("uuid").is_some_and(Value::is_string) => {
                let uuid = line["uuid"].as_str().unwrap().to_string();
                match nodes.get_mut(&uuid) {
                    Some(existing) => {
                        let changed = existing.line.get("message") != line.get("message")
                            || existing.line.get("parentUuid") != line.get("parentUuid");
                        if changed {
                            tally.rewrites_changed_content += 1;
                        }
                        existing.line = line;
                    }
                    None => {
                        order.push(uuid.clone());
                        nodes.insert(uuid, Node { line });
                    }
                }
            }
            _ => other_lines.push(line),
        }
    }

    // The conversation id is the file's own path under the store, not the
    // session id alone: a subagent transcript carries its parent's session id,
    // and ids are unique within a document.
    let id = file
        .path
        .strip_suffix(".jsonl")
        .unwrap_or(&file.path)
        .to_string();
    if nodes.is_empty() {
        warnings.note_for(WarningCode::ItemSkipped, Severity::Dropped, &id);
        return None;
    }
    if unreadable > 0 {
        let mut w = Warning::new(WarningCode::ItemSkipped, Severity::Dropped)
            .for_conversation(&id)
            .with_detail("lines that are not valid JSON");
        w.count = unreadable;
        warnings.push(w);
    }

    let mut conversation = Conversation::new(&id);
    // Both titles come from the source. The user's own wins over the one the
    // tool generated, and which was used is recorded, because a consumer may
    // well care that a title was written by a model.
    let (title, title_source) = match (custom_title, ai_title) {
        (Some(t), _) => (Some(t), Some("custom")),
        (None, Some(t)) => (Some(t), Some("ai")),
        (None, None) => (None, None),
    };
    conversation.title = title;

    let first = &nodes[&order[0]].line;
    if let Some(cwd) = first.get("cwd").and_then(Value::as_str) {
        // The working directory is the project. It is what the store itself is
        // keyed on.
        conversation.project = Some(ProjectRef {
            id: cwd.to_string(),
            name: None,
        });
    }
    let sidechain = first.get("isSidechain").and_then(Value::as_bool) == Some(true);
    conversation.x.insert(
        "x-panchat".into(),
        serde_json::json!({
            "claude_code_session_id": session_id,
            "claude_code_sidechain": sidechain,
            "claude_code_title_source": title_source,
            "claude_code_git_branch": first.get("gitBranch"),
        }),
    );

    // tool_use ids to tool names, so a tool_result can say which tool it
    // answers. The vendor links them by id; this follows the link rather than
    // guessing.
    let mut tool_names: HashMap<String, String> = HashMap::new();
    for uuid in &order {
        if let Some(blocks) = nodes[uuid]
            .line
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        {
            for b in blocks {
                if b.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let (Some(i), Some(n)) = (
                        b.get("id").and_then(Value::as_str),
                        b.get("name").and_then(Value::as_str),
                    ) {
                        tool_names.insert(i.to_string(), n.to_string());
                    }
                }
            }
        }
    }

    let present: HashSet<&str> = order.iter().map(String::as_str).collect();
    let mut parents: HashMap<String, Option<String>> = HashMap::new();
    let mut dangling = 0u32;
    let mut times: Vec<String> = Vec::new();

    for uuid in &order {
        let line = &nodes[uuid].line;
        let kind = line.get("type").and_then(Value::as_str).unwrap_or_default();
        let parent = line
            .get("parentUuid")
            .and_then(Value::as_str)
            // A compaction starts a new root that names where it came from.
            .or_else(|| line.get("logicalParentUuid").and_then(Value::as_str))
            .map(str::to_string);
        let parent = match parent {
            Some(p) if present.contains(p.as_str()) => Some(p),
            Some(_) => {
                dangling += 1;
                None
            }
            None => None,
        };
        parents.insert(uuid.clone(), parent.clone());

        let mut message = match kind {
            "user" | "assistant" => chat_message(uuid, line, &tool_names, tally),
            // Nodes that are part of the chain without being turns. Kept, and
            // hidden, so every message after one still has a parent that exists.
            other => {
                let mut m = Message::new(uuid.as_str(), Role::parse(other));
                if other == "system" {
                    m.role = Role::System;
                }
                m.hidden = true;
                m.content.push(ContentPart::Unknown {
                    kind: Some(other.to_string()),
                    raw: line.clone(),
                });
                m
            }
        };
        message.parent = parent;
        if let Some(t) = line.get("timestamp").and_then(Value::as_str) {
            message.created_at = Some(t.to_string());
            times.push(t.to_string());
        }
        if matches!(kind, "user" | "assistant") {
            tally.envelope_lines += 1;
        }
        conversation.messages.push(message);
    }

    if dangling > 0 {
        let mut w = Warning::new(WarningCode::BranchPointerBroken, Severity::Lossy)
            .for_conversation(&id)
            .with_detail("a node named a parent that is not in the session; it was made a root");
        w.count = dangling;
        warnings.push(w);
    }

    times.sort();
    conversation.created_at = times.first().cloned();
    conversation.updated_at = times.last().cloned();

    conversation.active_path = active_path(&id, leaf, &order, &parents, &nodes, warnings);
    conversation.raw = Some(Value::Array(other_lines));
    Some(conversation)
}

/// A user or assistant line as a message.
fn chat_message(
    uuid: &str,
    line: &Value,
    tool_names: &HashMap<String, String>,
    tally: &mut Tally,
) -> Message {
    let msg = &line["message"];
    let role = Role::parse(msg.get("role").and_then(Value::as_str).unwrap_or("user"));
    let mut m = Message::new(uuid, role);
    m.model = msg.get("model").and_then(Value::as_str).map(str::to_string);
    // Framing the tool injected rather than something the user typed.
    m.hidden = line.get("isMeta").and_then(Value::as_bool) == Some(true);

    match msg.get("content") {
        Some(Value::String(s)) if !s.is_empty() => {
            m.content.push(ContentPart::Text { text: s.clone() })
        }
        Some(Value::Array(blocks)) => {
            for b in blocks {
                let kind = b.get("type").and_then(Value::as_str).unwrap_or_default();
                match kind {
                    "text" => {
                        if let Some(t) = b.get("text").and_then(Value::as_str) {
                            m.content.push(ContentPart::Text {
                                text: t.to_string(),
                            });
                        }
                    }
                    "tool_use" => m.content.push(ContentPart::ToolUse {
                        name: b.get("name").and_then(Value::as_str).map(str::to_string),
                        input: b.get("input").cloned(),
                    }),
                    "tool_result" => m.content.push(ContentPart::ToolResult {
                        name: b
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .and_then(|i| tool_names.get(i))
                            .cloned(),
                        output: b.get("content").cloned(),
                    }),
                    // Reasoning is not the answer, and an image block carries
                    // its bytes inline in a form the representation has no
                    // field for. Both kept exactly as they were.
                    other => {
                        *tally.unknown_parts.entry(other.to_string()).or_default() += 1;
                        m.content.push(ContentPart::Unknown {
                            kind: Some(other.to_string()),
                            raw: b.clone(),
                        });
                    }
                }
            }
        }
        _ => {}
    }
    m
}

/// The thread the user last saw: up from the `leafUuid` Claude Code recorded.
fn active_path(
    id: &str,
    leaf: Option<String>,
    order: &[String],
    parents: &HashMap<String, Option<String>>,
    nodes: &HashMap<String, Node>,
    warnings: &mut Warnings,
) -> Vec<String> {
    let has_child: HashSet<&str> = parents.values().flatten().map(String::as_str).collect();
    let leaves: Vec<&String> = order
        .iter()
        .filter(|u| !has_child.contains(u.as_str()))
        .collect();
    let start = match leaf.filter(|l| nodes.contains_key(l)) {
        Some(l) => l,
        // A transcript that never branched has one leaf, and the thread is not
        // a reconstruction — subagent transcripts carry no pointer and need
        // none.
        None if leaves.len() == 1 => leaves[0].clone(),
        None => {
            // Several leaves and no pointer to choose between them. The newest
            // is the reconstruction, and it is reported as one.
            warnings.note_for(WarningCode::BranchPointerBroken, Severity::Lossy, id);
            match leaves.iter().max_by_key(|u| {
                nodes[u.as_str()]
                    .line
                    .get("timestamp")
                    .and_then(Value::as_str)
            }) {
                Some(u) => (*u).clone(),
                None => return Vec::new(),
            }
        }
    };
    let mut path = Vec::new();
    let mut seen = HashSet::new();
    let mut cursor = Some(start);
    while let Some(u) = cursor {
        if !seen.insert(u.clone()) {
            warnings.note_for(WarningCode::BranchCycle, Severity::Lossy, id);
            break;
        }
        cursor = parents.get(&u).cloned().flatten();
        path.push(u);
    }
    path.reverse();
    path
}
