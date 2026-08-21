//! Export sinks.
//!
//! Deliberately three, not N. Writing a vendor's own export format back out is
//! pointless — no vendor imports its own export — so the sinks are the ones
//! other tools actually consume: the IR as JSON, Markdown for humans and git,
//! and JSONL for analysis and fine-tuning pipelines.

use crate::ir::{ContentPart, Conversation, Document, Role};
use serde::Serialize;

/// Whether to render only the branch the user last saw, or every branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branches {
    /// Only [`Conversation::active_path`]. What a reader expects.
    ActiveOnly,
    /// Every message, with off-path turns marked. Nothing hidden.
    All,
}

/// Pretty-printed IR.
pub fn to_json(doc: &Document) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(doc)
}

/// One conversation per line, as the IR. The streaming-friendly sink.
pub fn to_jsonl(doc: &Document) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    for c in &doc.conversations {
        out.push_str(&serde_json::to_string(c)?);
        out.push('\n');
    }
    Ok(out)
}

/// Flat `{role, content}` turns — the shape training and eval pipelines expect.
#[derive(Serialize)]
struct FlatTurn<'a> {
    conversation_id: &'a str,
    role: &'a str,
    content: String,
}

/// One message per line. Lossy by construction: non-text parts are omitted and
/// branch structure is discarded. Use when a pipeline wants turns, not threads.
pub fn to_turns_jsonl(doc: &Document, branches: Branches) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    for c in &doc.conversations {
        let messages: Vec<_> = match branches {
            Branches::ActiveOnly => c.active_messages(),
            Branches::All => c.messages.iter().collect(),
        };
        for m in messages {
            let text = m.text();
            if text.is_empty() {
                continue;
            }
            out.push_str(&serde_json::to_string(&FlatTurn {
                conversation_id: &c.id,
                role: m.role.as_str(),
                content: text,
            })?);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Human- and git-readable Markdown with a YAML frontmatter header.
pub fn to_markdown(conversation: &Conversation, branches: Branches) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str("type: ai-conversation\n");
    if let Some(t) = &conversation.title {
        out.push_str(&format!("title: {}\n", yaml_scalar(t)));
    }
    out.push_str(&format!("id: {}\n", yaml_scalar(&conversation.id)));
    if let Some(ts) = &conversation.created_at {
        out.push_str(&format!("created_at: {ts}\n"));
    }
    if let Some(p) = &conversation.project {
        out.push_str(&format!(
            "project: {}\n",
            yaml_scalar(p.name.as_deref().unwrap_or(&p.id))
        ));
    }
    out.push_str("---\n\n");

    if let Some(t) = &conversation.title {
        out.push_str(&format!("# {t}\n\n"));
    }

    let messages: Vec<_> = match branches {
        // Turns the vendor hid from the user stay out of the rendered
        // transcript. They are still in the representation — rendering is where
        // the choice belongs, not parsing.
        Branches::ActiveOnly => conversation
            .active_messages()
            .into_iter()
            .filter(|m| !m.hidden)
            .collect(),
        Branches::All => conversation.messages.iter().collect(),
    };
    let on_path: Vec<&str> = conversation
        .active_path
        .iter()
        .map(String::as_str)
        .collect();

    for m in messages {
        let label = match m.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::System => "System",
            Role::Tool => "Tool",
            Role::Other(ref s) => s,
        };
        let off_path =
            branches == Branches::All && !on_path.is_empty() && !on_path.contains(&m.id.as_str());
        if off_path {
            out.push_str(&format!("## {label} _(off-path branch)_\n\n"));
        } else {
            out.push_str(&format!("## {label}\n\n"));
        }
        for part in &m.content {
            match part {
                ContentPart::Text { text } => {
                    out.push_str(text);
                    out.push_str("\n\n");
                }
                ContentPart::Attachment { name, id, .. } => {
                    let label = name.as_deref().or(id.as_deref()).unwrap_or("file");
                    out.push_str(&format!(
                        "_[attachment: {label} — not included in export]_\n\n"
                    ));
                }
                ContentPart::ToolUse { name, .. } => {
                    out.push_str(&format!(
                        "_[tool call: {}]_\n\n",
                        name.as_deref().unwrap_or("unnamed")
                    ));
                }
                ContentPart::ToolResult { name, .. } => {
                    out.push_str(&format!(
                        "_[tool result: {}]_\n\n",
                        name.as_deref().unwrap_or("unnamed")
                    ));
                }
                ContentPart::Unknown { kind, .. } => {
                    out.push_str(&format!(
                        "_[unrendered part: {}]_\n\n",
                        kind.as_deref().unwrap_or("unknown")
                    ));
                }
            }
        }
    }
    out
}

/// Quote a YAML scalar only when it needs it.
fn yaml_scalar(s: &str) -> String {
    let needs_quotes = s.is_empty()
        || s.starts_with([
            '&', '*', '!', '|', '>', '%', '@', '`', '"', '\'', '#', '-', '?',
        ])
        || s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':');
    if needs_quotes {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}
