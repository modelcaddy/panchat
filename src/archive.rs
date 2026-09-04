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
//!
//! # Archives that contain archives
//!
//! A large enough account no longer receives one archive. The download is a zip
//! of zips: part archives holding the conversations, and further part archives
//! holding the attachment bytes. Read naively that is an export containing no
//! export, and the user is told their download is unrecognisable.
//!
//! So one level of nesting is followed, and exactly one. The test for when to
//! follow it is the shape of what was found, never a filename: if the outer
//! archive already yielded a JSON array, the payload is here and there is
//! nothing to look for deeper — an ordinary export that happens to contain a
//! zip the user once uploaded is left alone. Only when the outer archive
//! yielded no array at all — sidecars and part archives and nothing else — are
//! the parts opened, and what they contain is merged as though the user had
//! unpacked every part into one folder, which is the layout the rest of this
//! crate is written against.
//!
//! An archive nested two deep is not followed. A vendor splitting an export
//! across parts is a real layout; an archive inside one of those parts is
//! either a file the user uploaded or an attempt to make a small download cost
//! unbounded work.

use crate::adapters::ExportFile;
use crate::Error;
use std::io::{Read, Seek};
use std::path::Path;

/// Total decompressed bytes this module will hold in memory for one archive.
///
/// An archive is the one input that can lie about its size: a few hundred
/// kilobytes of zip can claim to be terabytes of JSON. Referenced entries cost
/// nothing regardless, so this bounds only what is actually loaded.
pub(crate) const LOAD_BUDGET: u64 = 2 * 1024 * 1024 * 1024;

/// How many levels of nested archive are followed. See the module docs: one,
/// and no further.
const MAX_DEPTH: u32 = 1;

/// Read a zip archive from disk into the same shape [`crate::read_path`]
/// produces for a directory.
pub fn read_zip(path: &Path) -> Result<Vec<ExportFile>, Error> {
    let mut budget = LOAD_BUDGET;
    read_zip_with(path, &mut budget)
}

/// Read an archive against a budget somebody else is keeping, for a caller
/// opening several archives that are one export between them.
pub(crate) fn read_zip_with(path: &Path, budget: &mut u64) -> Result<Vec<ExportFile>, Error> {
    let file = std::fs::File::open(path)?;
    read_archive(file, 0, budget)
}

/// Read a zip archive already held in memory.
pub fn read_zip_bytes(bytes: &[u8]) -> Result<Vec<ExportFile>, Error> {
    let mut budget = LOAD_BUDGET;
    read_archive(std::io::Cursor::new(bytes), 0, &mut budget)
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
    // One budget across the whole slice, not one each: several archives handed
    // in together are one export, and the point of the budget is what this call
    // will hold in memory.
    let mut budget = LOAD_BUDGET;
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        if f.loaded && crate::is_zip(&f.bytes) {
            // Name the archive. The caller handed over one file and needs to
            // know it was this one that could not be opened.
            let read = read_archive(std::io::Cursor::new(&f.bytes), 0, &mut budget);
            out.extend(read.map_err(|e| match e {
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

fn read_archive<R: Read + Seek>(
    reader: R,
    depth: u32,
    budget: &mut u64,
) -> Result<Vec<ExportFile>, Error> {
    let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;

    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.name_for_index(i).map(str::to_string))
        .collect();
    let strip = common_root(&names);

    let mut out = Vec::new();
    // Indices of entries whose bytes were not read, kept so a second pass can
    // look for part archives among them without walking the whole archive again.
    let mut unread = Vec::new();
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
            unread.push((i, out.len()));
            out.push(ExportFile::reference(rel, size));
            continue;
        }

        // The declared size is the archive's claim about itself, so the read is
        // bounded by what is left of the budget rather than by that claim.
        let mut bytes = Vec::new();
        let read = entry
            .by_ref()
            .take(*budget + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Malformed(format!("could not read {rel} from the archive: {e}")))?
            as u64;
        if read > *budget {
            return Err(over_budget(&rel));
        }
        *budget -= read;
        out.push(ExportFile::new(rel, bytes));
    }

    if depth < MAX_DEPTH && !unread.is_empty() && !holds_a_json_array(&out) {
        expand_parts(&mut archive, &mut out, &unread, depth, budget)?;
    }
    Ok(out)
}

/// Whether anything read out of this archive is a JSON array.
///
/// The cheapest honest test for "the conversations are in here". Every export
/// this crate knows states its payload as a top-level array — ChatGPT's
/// conversation shards, Claude's `conversations.json` — while the files that
/// surround them (manifests, asset-name maps, memory side-cars) are objects.
/// So an archive that yielded an array needs nothing opened deeper, and an
/// archive that yielded only objects has its payload somewhere else.
///
/// Being wrong in the safe direction costs a user nothing: a false "yes" leaves
/// today's behaviour exactly as it was.
pub(crate) fn holds_a_json_array(files: &[ExportFile]) -> bool {
    files.iter().any(|f| {
        f.loaded
            && f.path.to_ascii_lowercase().ends_with(".json")
            && f.bytes
                .iter()
                .find(|b| !b.is_ascii_whitespace())
                .is_some_and(|b| *b == b'[')
    })
}

/// Open the part archives inside this one and merge what they hold.
///
/// Each part is read as though the user had unpacked it into the export root,
/// because that is the layout everything downstream is written against, and
/// because `ContentPart::Attachment.path` has to mean the same thing whether or
/// not anybody unpacked anything. The part archive itself is not a file of the
/// export, so its entry is replaced by its contents rather than kept beside
/// them.
///
/// Two parts may each carry a file of the same name — a manifest, say. Both are
/// kept: dropping one would be this crate deciding which of two things a vendor
/// shipped is the real one, and nothing here knows that.
fn expand_parts<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    out: &mut Vec<ExportFile>,
    unread: &[(usize, usize)],
    depth: u32,
    budget: &mut u64,
) -> Result<(), Error> {
    // Collected before splicing, since splicing moves every later index.
    let mut expanded: Vec<(usize, Vec<ExportFile>)> = Vec::new();
    for (index, slot) in unread {
        let mut entry = archive.by_index(*index).map_err(zip_error)?;
        let mut head = [0u8; 4];
        // A part archive is recognised by its magic, not by being called `.zip`.
        // Reading four bytes off the front of an entry is nearly free even when
        // it is compressed.
        let peeked = entry.read(&mut head).unwrap_or(0);
        if peeked < 4 || !crate::is_zip(&head) {
            continue;
        }

        let path = out[*slot].path.clone();
        let mut bytes = head[..peeked].to_vec();
        let read = entry
            .take(budget.saturating_sub(peeked as u64) + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Malformed(format!("could not read {path} from the archive: {e}")))?
            as u64
            + peeked as u64;
        if read > *budget {
            return Err(over_budget(&path));
        }
        *budget -= read;

        let inner =
            read_archive(std::io::Cursor::new(&bytes), depth + 1, budget).map_err(|e| match e {
                Error::Malformed(detail) => {
                    Error::Malformed(format!("{path}, an archive inside this one: {detail}"))
                }
                other => other,
            })?;
        // The part's own bytes were a means to its contents and are not held
        // any longer, so the budget they cost is returned to the pool.
        *budget += read;
        expanded.push((*slot, inner));
    }

    for (slot, inner) in expanded.into_iter().rev() {
        out.splice(slot..slot + 1, inner);
    }
    Ok(())
}

/// The archive claims to hold more than this process agreed to hold.
///
/// Worth naming the entry: with parts inside parts, "the archive" is ambiguous
/// and the user needs to know which file to go and look at.
fn over_budget(path: &str) -> Error {
    Error::Malformed(format!(
        "reading {path} would take this archive past {} GiB of loaded files; refusing to go \
         further (unpack the export and pass the folder if this is genuine)",
        LOAD_BUDGET / (1024 * 1024 * 1024)
    ))
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
