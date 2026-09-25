//! Building a document from a live capture.
//!
//! An export and a capture are lossy in opposite directions (SPEC.md,
//! *Acquisition methods*). An export ships the whole branch graph; a page
//! renders one branch, so a capture cannot see an answer the user regenerated
//! away. A consumer looking at a branch-free conversation has to be able to
//! tell "the user never regenerated" from "nobody could see", which is why a
//! capture producer MUST say so with `branches_unavailable`.
//!
//! A rule that depends on every producer remembering it is the first rule to
//! rot, and this one had no producer in this crate to exercise it. So it is
//! built in here instead: a [`Capture`] is the only way to start a capture
//! document this module offers, and it cannot be finished without the warning.
//!
//! ```
//! use panchat::capture::Capture;
//! use panchat::{ContentPart, Conversation, Message, Role};
//!
//! let mut capture = Capture::new("gemini", "web_capture_v1");
//! let mut c = Conversation::new("abc");
//! for (i, (role, text)) in [(Role::User, "hi"), (Role::Assistant, "hello")].into_iter().enumerate() {
//!     let mut m = Message::new(format!("m{i}"), role);
//!     m.content.push(ContentPart::Text { text: text.into() });
//!     c.messages.push(m);
//! }
//! capture.push(c);
//! let doc = capture.finish();
//! assert!(doc.warnings.iter().any(|w| w.code == panchat::WarningCode::BranchesUnavailable));
//! ```

use crate::ir::{Conversation, Document, Source};
use crate::warning::{Severity, Warning, WarningCode};

/// A document being assembled from what a page rendered, or from a client's
/// local history, one conversation at a time.
pub struct Capture {
    doc: Document,
    captured: u32,
}

impl Capture {
    /// Start a capture document. `method` is set to `capture`; that is the
    /// point of the type.
    pub fn new(platform: impl Into<String>, variant: impl Into<String>) -> Self {
        Self {
            doc: Document::new(Source::capture(platform, variant)),
            captured: 0,
        }
    }

    /// Add a conversation whose messages are in the order they were rendered.
    ///
    /// A page shows one linear thread, so that order *is* the thread: each
    /// message without a parent is linked to the one before it, and the active
    /// path is every message. A parent the caller did set is left alone — a
    /// client that stores its own graph knows better than rendering order —
    /// and so is an active path the caller already filled in.
    pub fn push(&mut self, mut conversation: Conversation) {
        let mut previous: Option<String> = None;
        for message in &mut conversation.messages {
            if message.parent.is_none() {
                message.parent.clone_from(&previous);
            }
            previous = Some(message.id.clone());
        }
        if conversation.active_path.is_empty() {
            conversation.active_path = conversation.messages.iter().map(|m| m.id.clone()).collect();
        }
        self.captured += 1;
        self.doc.conversations.push(conversation);
    }

    /// Record a warning for something else the capture could not see.
    pub fn warn(&mut self, warning: Warning) {
        self.doc.warnings.push(warning);
    }

    /// The document, with `branches_unavailable` on the record.
    ///
    /// Emitted once, folded, with a count of the conversations it covers: it is
    /// true of every one of them for the same reason, and ten thousand
    /// identical lines would bury everything else the document has to say.
    pub fn finish(mut self) -> Document {
        let mut w = Warning::new(WarningCode::BranchesUnavailable, Severity::Lossy).with_detail(
            "captured from what was rendered; answers regenerated or edited away were not visible",
        );
        w.count = self.captured.max(1);
        self.doc.warnings.insert(0, w);
        self.doc
    }
}
