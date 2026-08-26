//! Reading an export without unpacking it first.
//!
//! Every vendor ships its export as a zip, so "unpack it and pass the folder"
//! is a step this crate imposes on every user for its own convenience. With the
//! `zip` feature on, an archive is read in place and behaves exactly like the
//! unpacked directory would: the same relative paths, the same decision about
//! which files are worth loading, and the same [`ExportFile`] slice handed to
//! the same adapters. Nothing about the non-archive path changes.
//!
//! Entry paths are made to match the unpacked layout. Vendors wrap everything
//! in one top-level folder, and an archive that does is stripped of it, so
//! `ContentPart::Attachment.path` means the same thing either way.

use crate::adapters::ExportFile;
use crate::Error;
use std::io::{Read, Seek};
use std::path::Path;

/// Total decompressed bytes this module will hold in memory for one archive.
///
/// An archive is the one input that can lie about its size: a few hundred
/// kilobytes of zip can claim to be terabytes of JSON. Referenced entries cost
/// nothing regardless, so this bounds only what is actually loaded.
const LOAD_BUDGET: u64 = 2 * 1024 * 1024 * 1024;

/// Read a zip archive from disk into the same shape [`crate::read_path`]
/// produces for a directory.
pub fn read_zip(path: &Path) -> Result<Vec<ExportFile>, Error> {
    let file = std::fs::File::open(path)?;
    read_archive(file)
}

/// Read a zip archive already held in memory.
pub fn read_zip_bytes(bytes: &[u8]) -> Result<Vec<ExportFile>, Error> {
    read_archive(std::io::Cursor::new(bytes))
}

/// Replace any zip in `files` with its contents.
///
/// Returns `None` when there is nothing to expand, so the common case does not
/// copy the slice. A zip nested inside an export is left alone — only an
/// archive handed in at the top level is opened, and expanding recursively
/// would let a crafted export cost unbounded work.
pub fn expand_archives(files: &[ExportFile]) -> Result<Option<Vec<ExportFile>>, Error> {
    if !files.iter().any(|f| f.loaded && crate::is_zip(&f.bytes)) {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        if f.loaded && crate::is_zip(&f.bytes) {
            // Name the archive. The caller handed over one file and needs to
            // know it was this one that could not be opened.
            out.extend(read_zip_bytes(&f.bytes).map_err(|e| match e {
                Error::Malformed(detail) => {
                    Error::Malformed(format!("{} is a zip archive: {detail}", f.path))
                }
                other => other,
            })?);
        } else {
            out.push(f.clone());
        }
    }
    Ok(Some(out))
}

fn read_archive<R: Read + Seek>(reader: R) -> Result<Vec<ExportFile>, Error> {
    let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;

    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.name_for_index(i).map(str::to_string))
        .collect();
    let strip = common_root(&names);

    let mut out = Vec::new();
    let mut budget = LOAD_BUDGET;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(zip_error)?;
        if entry.is_dir() {
            continue;
        }
        // `enclosed_name` rejects `..` and absolute paths. A traversal entry
        // cannot hurt us — nothing here writes to disk — but it would put a
        // nonsense path in the output, so it is skipped rather than trusted.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy().replace('\\', "/");
        let rel = strip
            .as_deref()
            .and_then(|p| name.strip_prefix(p))
            .unwrap_or(&name)
            .to_string();
        if rel.is_empty() {
            continue;
        }

        let size = entry.size();
        if !crate::should_load(&rel, size) {
            out.push(ExportFile::reference(rel, size));
            continue;
        }

        // The declared size is the archive's claim about itself, so the read is
        // bounded by what is left of the budget rather than by that claim.
        let mut bytes = Vec::new();
        let read = entry
            .by_ref()
            .take(budget + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Malformed(format!("could not read {rel} from the archive: {e}")))?
            as u64;
        if read > budget {
            return Err(Error::Malformed(format!(
                "archive expands to more than {} GiB of readable files; refusing to load it \
                 (unpack it and pass the folder if this is genuine)",
                LOAD_BUDGET / (1024 * 1024 * 1024)
            )));
        }
        budget -= read;
        out.push(ExportFile::new(rel, bytes));
    }
    Ok(out)
}

/// The single top-level directory every entry sits under, if there is one.
///
/// Returned with its trailing slash, so stripping it is a plain prefix cut.
fn common_root(names: &[String]) -> Option<String> {
    let mut root: Option<String> = None;
    let mut saw_file = false;
    for name in names {
        let normalized = name.replace('\\', "/");
        // A directory entry names the folder itself; the files under it decide.
        if normalized.ends_with('/') {
            continue;
        }
        let (head, rest) = normalized.split_once('/')?;
        if head.is_empty() || rest.is_empty() {
            return None;
        }
        saw_file = true;
        match &root {
            None => root = Some(head.to_string()),
            Some(r) if r == head => {}
            Some(_) => return None,
        }
    }
    if !saw_file {
        return None;
    }
    root.map(|r| format!("{r}/"))
}

/// A corrupt or unsupported archive is a malformed export, not an IO failure:
/// the file was readable, its contents were not.
fn zip_error(e: zip::result::ZipError) -> Error {
    Error::Malformed(format!("could not read the zip archive: {e}"))
}
