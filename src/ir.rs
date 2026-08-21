//! The intermediate representation every adapter produces.
//!
//! Design rules this module is bound by (`docs/OPEN_CHAT_INTERCHANGE.md` §5):
//!
//! 1. **Tiny required core.** Only `id` and `messages` are required on a
//!    conversation; only `id` and `role` on a message. Everything a given
//!    vendor may omit is `Option`.
//! 2. **Extension points reserved from v0.1.** Every struct carries an `x` map
//!    for namespaced third-party keys, and conversations carry `raw` for the
//!    untouched vendor payload.
//! 3. **Unknown shapes are preserved, never dropped** — that is what `x`,
//!    `raw`, and [`ContentPart::Unknown`] exist for.
//! 4. **Nothing ModelCaddy-specific in the core namespace.** ModelCaddy's own
//!    concepts belong under an `x-modelcaddy` key.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Current schema version emitted by this crate. `0.x` signals that the shape
/// may still move; see `docs/OPEN_CHAT_INTERCHANGE.md` §5.
pub const FORMAT_VERSION: &str = "0.1";

/// Canonical URL of the JSON Schema for [`FORMAT_VERSION`].
pub const SCHEMA_URL: &str = "https://modelcaddy.github.io/panchat/schema/chat-v0.1.json";

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_empty_map(m: &BTreeMap<String, serde_json::Value>) -> bool {
    m.is_empty()
}

/// A parsed export: one or more conversations plus everything we could not
/// faithfully represent.
///
/// Warnings live on the document rather than being returned alongside it, so a
/// serialized file still carries its own honesty record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// Schema this document claims to conform to.
    pub schema: String,
    pub format_version: String,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conversations: Vec<Conversation>,
    /// Non-conversation items a vendor ships alongside chats (Claude projects
    /// and memories, ChatGPT custom instructions). Deliberately loose —
    /// modelling every vendor's side-cars is not a v0.1 goal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<crate::warning::Warning>,
    #[serde(default, skip_serializing_if = "is_empty_map", flatten)]
    pub x: BTreeMap<String, serde_json::Value>,
}

impl Document {
    pub fn new(source: Source) -> Self {
        Self {
            schema: SCHEMA_URL.to_string(),
            format_version: FORMAT_VERSION.to_string(),
            source,
            conversations: Vec::new(),
            artifacts: Vec::new(),
            warnings: Vec::new(),
            x: BTreeMap::new(),
        }
    }
}

/// How the data was obtained.
///
/// This is not bookkeeping: the two paths are lossy in *opposite* directions,
/// and a consumer cannot interpret the data without knowing which it holds. A
/// vendor export ships the full branch graph but arrives slowly and only exists
/// for some platforms. A live capture works on any platform with a web page and
/// is immediate and selective, but can only see what the page rendered — which
/// never includes regenerated-away branches.
///
/// So "no alternative branches" means "the user never regenerated" under
/// [`Method::Export`], and means "we could not see them" under
/// [`Method::Capture`]. Same data, different truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    /// The vendor's own data export.
    Export,
    /// Read from a live page or a client's local storage.
    Capture,
    #[serde(untagged)]
    Other(String),
}

/// Where this document came from. `platform` is an open string, not an enum:
/// an adapter for a vendor we have never heard of must still be expressible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub platform: String,
    /// How the data was obtained. Absent means unknown, which a consumer
    /// should treat as the weaker guarantee.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<Method>,
    /// Which shape was recognised, e.g. `official_export_v1`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
}

impl Source {
    /// A vendor data export.
    pub fn new(platform: impl Into<String>, variant: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            method: Some(Method::Export),
            variant: Some(variant.into()),
            exported_at: None,
        }
    }

    /// A live capture — a browser extension reading a rendered page, or a
    /// client's on-disk history.
    pub fn capture(platform: impl Into<String>, variant: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            method: Some(Method::Capture),
            variant: Some(variant.into()),
            exported_at: None,
        }
    }
}

/// One conversation.
///
/// Messages are stored **flat with parent pointers**, not nested. A ChatGPT
/// export is a graph, not a list: regenerated and edited turns are siblings
/// under a shared parent. Flattening to the active path is lossy and
/// irreversible, so the graph is kept whole and [`Conversation::active_path`]
/// records which branch the vendor considered current. Consumers that only
/// want a linear transcript call [`Conversation::active_messages`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Vendor-side project/folder/space membership, when the export records it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub messages: Vec<Message>,
    /// Message ids, root-first, forming the branch the vendor marked current.
    /// Empty when the export gives no such signal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_path: Vec<String>,
    /// The untouched vendor payload for this conversation. Round-trip
    /// insurance: anything this crate failed to model is still in here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "is_empty_map", flatten)]
    pub x: BTreeMap<String, serde_json::Value>,
}

impl Conversation {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: None,
            created_at: None,
            updated_at: None,
            project: None,
            url: None,
            messages: Vec::new(),
            active_path: Vec::new(),
            raw: None,
            x: BTreeMap::new(),
        }
    }

    /// The messages on [`Conversation::active_path`], in order. Falls back to
    /// every message in stored order when no active path was recorded.
    pub fn active_messages(&self) -> Vec<&Message> {
        if self.active_path.is_empty() {
            return self.messages.iter().collect();
        }
        self.active_path
            .iter()
            .filter_map(|id| self.messages.iter().find(|m| &m.id == id))
            .collect()
    }

    /// Messages that exist but are not on the active path — regenerations,
    /// edited-away turns, abandoned branches.
    pub fn off_path_messages(&self) -> Vec<&Message> {
        if self.active_path.is_empty() {
            return Vec::new();
        }
        self.messages
            .iter()
            .filter(|m| !self.active_path.contains(&m.id))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRef {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    /// Parent message id. `None` marks a root. Together these edges carry the
    /// branch structure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Which model produced this message, when the export says. Per-message,
    /// not per-conversation: a single thread often spans several models.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ContentPart>,
    /// True when the vendor marks this turn as hidden from the user (system
    /// framing, tool plumbing). Recorded rather than dropped.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "is_empty_map", flatten)]
    pub x: BTreeMap<String, serde_json::Value>,
}

impl Message {
    pub fn new(id: impl Into<String>, role: Role) -> Self {
        Self {
            id: id.into(),
            parent: None,
            role,
            created_at: None,
            model: None,
            content: Vec::new(),
            hidden: false,
            x: BTreeMap::new(),
        }
    }

    /// Concatenated text of every [`ContentPart::Text`] part. Non-text parts
    /// contribute nothing — callers that care about them must inspect
    /// [`Message::content`] directly.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn has_non_text(&self) -> bool {
        self.content
            .iter()
            .any(|p| !matches!(p, ContentPart::Text { .. }))
    }
}

/// Open role. Not an exhaustive enum — vendors invent roles, and an
/// unrecognised one must survive rather than fail the parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
    #[serde(untagged)]
    Other(String),
}

impl Role {
    pub fn parse(s: &str) -> Self {
        match s {
            "user" | "human" => Role::User,
            "assistant" | "model" | "bot" => Role::Assistant,
            "system" => Role::System,
            "tool" | "function" => Role::Tool,
            other => Role::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
            Role::Other(s) => s,
        }
    }
}

/// A piece of a message. [`ContentPart::Unknown`] is load-bearing: a part shape
/// this crate does not recognise is carried through verbatim rather than
/// replaced with a placeholder, so an out-of-date parser degrades instead of
/// destroying data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    /// A file the conversation referenced. `path` is populated only when the
    /// bytes actually shipped inside the export; most exports omit them.
    Attachment {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        size_bytes: Option<u64>,
    },
    ToolUse {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<serde_json::Value>,
    },
    ToolResult {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<serde_json::Value>,
    },
    /// A part whose shape this version does not model. `kind` is the vendor's
    /// own label where one exists; `raw` is the untouched payload.
    Unknown {
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        raw: serde_json::Value,
    },
}

/// A non-conversation item shipped in the same export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    /// `project` | `memory` | `instruction` | vendor-specific.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}
