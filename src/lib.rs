#![doc = include_str!("../README.md")]

pub mod adapters;
pub mod export;
pub mod ir;
pub mod warning;

pub use adapters::{Adapter, Detection, ExportFile};
pub use ir::{
    Artifact, ContentPart, Conversation, Document, Message, Method, ProjectRef, Role, Source,
    FORMAT_VERSION, SCHEMA_URL,
};
pub use warning::{Severity, Warning, WarningCode, Warnings};

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No registered adapter recognised these files.
    #[error("unrecognised export: {0}")]
    NotRecognized(String),
    /// An adapter recognised the export but it is structurally unreadable.
    #[error("malformed export: {0}")]
    Malformed(String),
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Parse an already-loaded export.
///
/// Zero-config: the vendor is detected from the file shapes, never passed in.
/// Warnings ride along on the returned [`Document`] — a caller that ignores
/// them still gets a best-effort result, and a caller that reads them can tell
/// a user exactly what their export left behind.
///
/// ```no_run
/// # fn main() -> Result<(), panchat::Error> {
/// let files = panchat::read_path("chatgpt-export/")?;
/// let doc = panchat::normalize(&files)?;
/// for w in &doc.warnings {
///     eprintln!("{:?}: {}", w.severity, w.code.describe());
/// }
/// # Ok(())
/// # }
/// ```
pub fn normalize(files: &[ExportFile]) -> Result<Document, Error> {
    let (adapter, detection) = adapters::detect(files).ok_or_else(|| {
        Error::NotRecognized(format!(
            "no adapter matched {} file(s): {}",
            files.len(),
            files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })?;

    let mut warnings = Warnings::new();
    let mut doc = adapter.parse(files, &mut warnings)?;
    doc.source.variant = Some(detection.variant.to_string());
    doc.warnings = warnings.into_vec();
    Ok(doc)
}

/// Which adapter would handle these files, without parsing them.
pub fn detect(files: &[ExportFile]) -> Option<Detection> {
    adapters::detect(files).map(|(_, d)| d)
}

/// Read a file or a directory of files into memory.
///
/// A convenience for the common case, and deliberately the only filesystem
/// entry point — adapters themselves never touch the disk, which is what keeps
/// them fixture-testable.
///
/// Reads whole files into memory. Streaming for gigabyte-scale exports is
/// Phase 2 (`docs/OPEN_CHAT_INTERCHANGE.md` §6.4); until then a very large
/// `conversations.json` will use memory proportional to its size.
pub fn read_path(path: impl AsRef<Path>) -> Result<Vec<ExportFile>, Error> {
    let path = path.as_ref();
    if path.is_file() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        return Ok(vec![ExportFile::new(name, std::fs::read(path)?)]);
    }

    let mut out = Vec::new();
    collect_dir(path, path, &mut out)?;
    Ok(out)
}

fn collect_dir(root: &Path, dir: &Path, out: &mut Vec<ExportFile>) -> Result<(), Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            collect_dir(root, &p, out)?;
        } else {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(ExportFile::new(rel, std::fs::read(&p)?));
        }
    }
    Ok(())
}
