//! Structured lossiness reporting.
//!
//! The signature feature of this crate: every parse says out loud what it could
//! not faithfully represent. A caller that ignores warnings gets a best-effort
//! result; a caller that reads them can tell a user exactly what their vendor's
//! export left behind.

use serde::{Deserialize, Serialize};

/// A machine-readable reason a parse was lossy or incomplete.
///
/// Codes are stable identifiers — renaming one is a breaking change. New codes
/// are additive, so consumers must tolerate codes they do not recognise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    /// The export records no timestamps at all for this item.
    MissingTimestamps,
    /// The export records no stable id; one was synthesized.
    SynthesizedId,
    /// A message part shape was not recognised and was preserved verbatim as
    /// [`crate::ir::ContentPart::Unknown`] rather than interpreted.
    UnknownContentPart,
    /// The conversation references a file whose bytes are not in the export.
    AttachmentNotIncluded,
    /// The vendor's active-branch pointer was missing or broken; message order
    /// is a best-effort reconstruction.
    BranchPointerBroken,
    /// A cycle was found while walking parent pointers and the walk was cut.
    BranchCycle,
    /// An item was skipped entirely — malformed beyond recovery.
    ItemSkipped,
    /// The export contains a file or key this adapter version does not handle.
    UnhandledExportSection,
    /// Per-message model identity is absent from this export format.
    NoModelIdentity,
    /// The acquisition method cannot observe alternative branches. A live
    /// capture sees only what the page rendered, so regenerated-away answers
    /// are invisible — distinct from a source that genuinely has none.
    BranchesUnavailable,
}

impl WarningCode {
    /// Short, user-facing explanation. Deliberately plain — this text ends up
    /// in front of people who did not write the export.
    pub fn describe(&self) -> &'static str {
        match self {
            WarningCode::MissingTimestamps => "this export contains no timestamps",
            WarningCode::SynthesizedId => "no stable id in the export; one was generated",
            WarningCode::UnknownContentPart => {
                "a message part was not recognised and was kept verbatim"
            }
            WarningCode::AttachmentNotIncluded => {
                "an attachment is referenced but its bytes are not in the export"
            }
            WarningCode::BranchPointerBroken => {
                "the active-branch pointer was missing or broken; order is reconstructed"
            }
            WarningCode::BranchCycle => "a cycle in the message graph was cut",
            WarningCode::ItemSkipped => "an item was too malformed to parse and was skipped",
            WarningCode::UnhandledExportSection => {
                "part of the export is not handled by this adapter version"
            }
            WarningCode::NoModelIdentity => {
                "this export does not record which model produced each message"
            }
            WarningCode::BranchesUnavailable => {
                "regenerated or edited-away answers cannot be seen by this capture method"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Something is absent that never existed in the source. Nothing was lost.
    Info,
    /// Something in the source was not fully represented.
    Lossy,
    /// Something in the source was dropped.
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warning {
    pub code: WarningCode,
    pub severity: Severity,
    /// Which conversation this concerns, when it is item-specific.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// How many times this occurred, once warnings are folded.
    #[serde(default = "one")]
    pub count: u32,
    /// Free-text detail. Never the only carrier of meaning — `code` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

fn one() -> u32 {
    1
}

impl Warning {
    pub fn new(code: WarningCode, severity: Severity) -> Self {
        Self {
            code,
            severity,
            conversation_id: None,
            message_id: None,
            count: 1,
            detail: None,
        }
    }

    pub fn for_conversation(mut self, id: impl Into<String>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Accumulates warnings during a parse and folds repeats into counts, so a
/// 10,000-conversation export does not return 10,000 identical lines.
#[derive(Debug, Default)]
pub struct Warnings {
    inner: Vec<Warning>,
}

impl Warnings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a warning that is not tied to one conversation. Repeats of the
    /// same code are folded into a single entry with a count.
    pub fn note(&mut self, code: WarningCode, severity: Severity) {
        if let Some(existing) = self
            .inner
            .iter_mut()
            .find(|w| w.code == code && w.conversation_id.is_none())
        {
            existing.count += 1;
            return;
        }
        self.inner.push(Warning::new(code, severity));
    }

    /// Record a warning against a specific conversation. Kept unfolded — the
    /// conversation id is the point.
    pub fn note_for(
        &mut self,
        code: WarningCode,
        severity: Severity,
        conversation_id: impl Into<String>,
    ) {
        self.inner
            .push(Warning::new(code, severity).for_conversation(conversation_id));
    }

    pub fn push(&mut self, warning: Warning) {
        self.inner.push(warning);
    }

    pub fn into_vec(self) -> Vec<Warning> {
        self.inner
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}
