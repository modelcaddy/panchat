#![doc = include_str!("../README.md")]

pub mod adapters;
#[cfg(feature = "zip")]
pub mod archive;
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
    // An archive handed in whole is expanded here rather than at the filesystem
    // boundary, so a caller that loaded the bytes itself — over HTTP, out of a
    // database, from a browser upload — gets the same behaviour as one that
    // passed a path.
    #[cfg(feature = "zip")]
    let archives: Vec<String> = files
        .iter()
        .filter(|f| f.loaded && is_zip(&f.bytes))
        .map(|f| f.path.clone())
        .collect();
    #[cfg(feature = "zip")]
    let expanded = archive::expand_archives(files)?;
    #[cfg(feature = "zip")]
    let files: &[ExportFile] = expanded.as_deref().unwrap_or(files);

    let (adapter, detection) = adapters::detect(files).ok_or_else(|| {
        let err = unrecognized(files);
        // The user passed an archive, so the error has to name the archive.
        // After expansion the failing files are entries inside it, and an error
        // about `holiday.txt` when they handed over `export.zip` is a riddle.
        #[cfg(feature = "zip")]
        if let (Error::NotRecognized(detail), [name, ..]) = (&err, archives.as_slice()) {
            return Error::NotRecognized(format!(
                "{name} is a zip archive, and nothing inside it looked like an export: {detail}"
            ));
        }
        err
    })?;

    let mut warnings = Warnings::new();
    let mut doc = adapter.parse(files, &mut warnings)?;
    doc.source.variant = Some(detection.variant.to_string());
    doc.source.variant_version = Some(detection.variant_version);
    doc.warnings = warnings.into_vec();
    Ok(doc)
}

/// Why nothing matched. Vendors ship exports as zip archives and people pass
/// them along unopened, so "unrecognised export" is the wrong answer to the
/// commonest mistake: say what the file is and what to do about it.
fn unrecognized(files: &[ExportFile]) -> Error {
    if let Some(zip) = files.iter().find(|f| is_zip(&f.bytes)) {
        // With the `zip` feature on, an archive has already been expanded by
        // the time we get here, so reaching this means the archive was read and
        // its contents still matched no adapter.
        #[cfg(feature = "zip")]
        return Error::NotRecognized(format!(
            "{} is a zip archive, and nothing inside it looked like an export",
            zip.path
        ));
        #[cfg(not(feature = "zip"))]
        return Error::NotRecognized(format!(
            "{} is a zip archive; unpack it and pass the folder, or build with the `zip` feature \
             to read archives directly",
            zip.path
        ));
    }
    if files.iter().all(|f| !f.loaded) {
        return Error::NotRecognized(format!(
            "{} file(s), none of them readable as an export",
            files.len()
        ));
    }
    Error::NotRecognized(format!(
        "no adapter matched {} file(s): {}",
        files.len(),
        files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Local file header, or the end-of-central-directory record of an empty
/// archive.
pub(crate) fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
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
/// Structured files are read whole; attachment blobs are only *referenced*.
/// A 619 MB ChatGPT export is 66 MB of conversations and half a gigabyte of
/// images, audio, and a rendered `chat.html` — and no adapter reads a byte of
/// those, so loading them would cost the user most of a gigabyte of memory to
/// learn a filename. See [`ExportFile::reference`].
///
/// A path naming a single file is always read whole: the user pointed at it.
///
/// Streaming for gigabyte-scale exports is not implemented yet, so a very
/// large `conversations.json` still uses memory proportional to its size.
pub fn read_path(path: impl AsRef<Path>) -> Result<Vec<ExportFile>, Error> {
    let path = path.as_ref();
    if path.is_file() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let bytes = std::fs::read(path)?;
        // Detected by content, not by extension: vendors and browsers rename
        // downloads freely, and a zip is a zip whatever it is called.
        #[cfg(feature = "zip")]
        if is_zip(&bytes) {
            return archive::read_zip_bytes(&bytes);
        }
        return Ok(vec![ExportFile::new(name, bytes)]);
    }

    let mut out = Vec::new();
    collect_dir(path, path, &mut out)?;
    #[cfg(feature = "zip")]
    let out = expand_directory_archives(path, out)?;
    Ok(out)
}

/// A folder whose export is still inside the archives sitting in it.
///
/// Google Takeout splits a large account across numbered downloads, and what
/// somebody does with those is put them in one folder. Every archive is a blob
/// as far as the walk above is concerned — a `.zip` is not a structured file —
/// so without this the folder reads as several unopenable files and the user is
/// told their download is unrecognisable, which is the same wrong answer a zip
/// of zips used to get.
///
/// The rule is the one the archive reader already uses one level down: only
/// when nothing in the folder yielded a JSON array is the payload assumed to be
/// inside the archives. A folder holding an unpacked export *and* a zip is left
/// exactly as it was.
#[cfg(feature = "zip")]
fn expand_directory_archives(
    root: &Path,
    files: Vec<ExportFile>,
) -> Result<Vec<ExportFile>, Error> {
    if archive::holds_a_json_array(&files) {
        return Ok(files);
    }
    // One budget for the folder: several downloads of one account are one
    // export, and the budget is a claim about memory rather than about files.
    let mut budget = archive::LOAD_BUDGET;
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let full = root.join(&f.path);
        if !f.loaded && starts_with_zip_magic(&full) {
            out.extend(
                archive::read_zip_with(&full, &mut budget).map_err(|e| match e {
                    Error::Malformed(detail) => {
                        Error::Malformed(format!("{} is a zip archive: {detail}", f.path))
                    }
                    other => other,
                })?,
            );
        } else {
            out.push(f);
        }
    }
    Ok(out)
}

/// Whether the file on disk is an archive, without reading it.
#[cfg(feature = "zip")]
fn starts_with_zip_magic(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4];
    match file.read_exact(&mut head) {
        Ok(()) => is_zip(&head),
        Err(_) => false,
    }
}

/// Extensions worth reading. Every vendor export we know states its structure
/// in JSON; the rest are sidecars small enough that reading them is free.
const STRUCTURED: [&str; 5] = ["json", "jsonl", "ndjson", "md", "txt"];

/// A file with no extension might be a renamed export, so it is read anyway —
/// up to a size at which that guess stops being worth the memory.
const UNTYPED_LIMIT: u64 = 4 * 1024 * 1024;

/// Whether a file at this relative path is worth loading, or should be recorded
/// by name and size alone.
///
/// Shared with the archive reader so that a zip and the folder it unpacks to
/// produce the same [`ExportFile`] slice.
pub(crate) fn should_load(rel_path: &str, size: u64) -> bool {
    // `Path::extension` rather than a manual split, so that a dotfile is
    // extensionless here exactly as it is when walking a directory.
    match Path::new(rel_path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
    {
        Some(ext) => STRUCTURED.contains(&ext.as_str()),
        None => size <= UNTYPED_LIMIT,
    }
}

fn collect_dir(root: &Path, dir: &Path, out: &mut Vec<ExportFile>) -> Result<(), Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            collect_dir(root, &p, out)?;
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if should_load(&rel, size) {
            out.push(ExportFile::new(rel, std::fs::read(&p)?));
        } else {
            out.push(ExportFile::reference(rel, size));
        }
    }
    Ok(())
}
