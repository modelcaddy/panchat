//! Vendor adapters.
//!
//! An adapter is pure with respect to its inputs: given the files of an export
//! it votes on whether it recognises them, and parses them into the IR. It
//! never touches the filesystem, a database, or a clock. That is what makes
//! adapters fixture-testable, and fixture tests are the only thing standing
//! between us and silent rot when a vendor changes an export.

use crate::ir::Document;
use crate::warning::Warnings;
use crate::Error;

pub mod chatgpt;
pub mod claude;

/// One file from an export. A bare `conversations.json` is a single-element
/// slice; an unpacked archive is many.
#[derive(Debug, Clone)]
pub struct ExportFile {
    /// Path relative to the export root, using forward slashes.
    pub path: String,
    pub bytes: Vec<u8>,
}

impl ExportFile {
    pub fn new(path: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            bytes,
        }
    }

    /// Lowercased path, for case-insensitive matching.
    pub fn lower_path(&self) -> String {
        self.path.to_ascii_lowercase()
    }
}

/// An adapter's vote that it recognises an export.
#[derive(Debug, Clone)]
pub struct Detection {
    pub platform: &'static str,
    pub variant: &'static str,
    /// 0.0–1.0. The registry picks the highest vote; ties break on registry
    /// order, so keep the list in descending order of shape distinctiveness.
    pub confidence: f64,
    pub notes: Vec<String>,
}

pub trait Adapter: Send + Sync {
    fn platform(&self) -> &'static str;
    fn variant(&self) -> &'static str;

    /// Return a vote if these files look like this adapter's export shape.
    fn detect(&self, files: &[ExportFile]) -> Option<Detection>;

    /// Parse into the IR, recording lossiness in `warnings`.
    ///
    /// Adapters return `Err` only when the input is not theirs or is
    /// structurally unreadable. A malformed *item* inside a readable export is
    /// a [`crate::warning::WarningCode::ItemSkipped`] warning, not an error —
    /// one bad conversation must never cost the user the other 9,999.
    fn parse(&self, files: &[ExportFile], warnings: &mut Warnings) -> Result<Document, Error>;
}

/// Every registered adapter, in detection priority order.
pub fn all() -> Vec<Box<dyn Adapter>> {
    vec![Box::new(chatgpt::ChatGpt), Box::new(claude::Claude)]
}

/// Highest-confidence adapter for these files, if any recognises them.
pub fn detect(files: &[ExportFile]) -> Option<(Box<dyn Adapter>, Detection)> {
    let mut best: Option<(Box<dyn Adapter>, Detection)> = None;
    for adapter in all() {
        if let Some(d) = adapter.detect(files) {
            let better = best
                .as_ref()
                .map(|(_, prev)| d.confidence > prev.confidence)
                .unwrap_or(true);
            if better {
                best = Some((adapter, d));
            }
        }
    }
    best
}
