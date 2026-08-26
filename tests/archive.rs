//! Reading an export straight out of the vendor's zip.
//!
//! The property under test is equivalence: an archive and the folder it unpacks
//! to must produce the same document, because everything downstream — adapters,
//! attachment paths, warnings — was written against the folder.

#![cfg(feature = "zip")]

use panchat::ExportFile;
use std::io::Write;

const CHATGPT: &str = include_str!("fixtures/chatgpt_branched.json");

/// Build a zip in memory. Stored, not deflated: these are fixtures, and the
/// compression method is not what is under test.
fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            w.start_file(*name, options).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

fn unpacked() -> Vec<ExportFile> {
    vec![ExportFile::new(
        "conversations.json",
        CHATGPT.as_bytes().to_vec(),
    )]
}

#[test]
fn zip_and_unpacked_folder_produce_the_same_document() {
    let archive = zip_of(&[("conversations.json", CHATGPT.as_bytes())]);
    let from_zip = panchat::normalize(&[ExportFile::new("export.zip", archive)]).unwrap();
    let from_folder = panchat::normalize(&unpacked()).unwrap();

    assert_eq!(
        serde_json::to_value(&from_zip).unwrap(),
        serde_json::to_value(&from_folder).unwrap(),
        "an archive must normalize identically to the folder it unpacks to"
    );
}

#[test]
fn a_wrapper_directory_is_stripped_so_paths_match_the_unpacked_layout() {
    let archive = zip_of(&[
        ("chatgpt-export/conversations.json", CHATGPT.as_bytes()),
        (
            "chatgpt-export/dalle-generations/img.png",
            b"\x89PNG binary",
        ),
    ]);
    let files = panchat::archive::read_zip_bytes(&archive).unwrap();

    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.contains(&"conversations.json"),
        "the vendor's wrapper folder should not appear in entry paths: {paths:?}"
    );
    assert!(paths.contains(&"dalle-generations/img.png"), "{paths:?}");
}

#[test]
fn a_shared_prefix_that_is_not_a_wrapper_directory_is_kept() {
    // Two roots means no wrapper: stripping either would be a guess.
    let archive = zip_of(&[
        ("a/conversations.json", CHATGPT.as_bytes()),
        ("b/notes.txt", b"hello"),
    ]);
    let files = panchat::archive::read_zip_bytes(&archive).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(paths.contains(&"a/conversations.json"), "{paths:?}");
    assert!(paths.contains(&"b/notes.txt"), "{paths:?}");
}

#[test]
fn attachment_blobs_inside_an_archive_are_referenced_not_loaded() {
    let archive = zip_of(&[
        ("conversations.json", CHATGPT.as_bytes()),
        ("audio/voice.wav", &vec![0u8; 4096]),
    ]);
    let files = panchat::archive::read_zip_bytes(&archive).unwrap();

    let wav = files.iter().find(|f| f.path == "audio/voice.wav").unwrap();
    assert!(!wav.loaded, "a .wav is an attachment blob, not structure");
    assert!(wav.bytes.is_empty(), "referenced files carry no bytes");
    assert_eq!(wav.size_bytes, 4096, "the real size is still reported");

    let convs = files
        .iter()
        .find(|f| f.path == "conversations.json")
        .unwrap();
    assert!(convs.loaded);
}

#[test]
fn a_corrupt_archive_is_a_malformed_export_not_an_io_error() {
    // Zip magic, then garbage: recognised as an archive, unreadable as one.
    let mut bytes = b"PK\x03\x04".to_vec();
    bytes.extend_from_slice(b"not actually a zip file");
    let err = panchat::normalize(&[ExportFile::new("export.zip", bytes)]).unwrap_err();

    assert!(
        matches!(err, panchat::Error::Malformed(_)),
        "expected Malformed, got {err:?}"
    );
}

#[test]
fn an_archive_of_something_else_says_so() {
    let archive = zip_of(&[("holiday.txt", b"not an export")]);
    let err = panchat::normalize(&[ExportFile::new("export.zip", archive)]).unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("nothing inside it looked like an export"),
        "the error should say the archive was opened and read: {message}"
    );
}

#[test]
fn a_non_archive_input_is_untouched() {
    // The regression that matters: turning the feature on must not change the
    // behaviour of every export that is not a zip.
    let doc = panchat::normalize(&unpacked()).unwrap();
    assert_eq!(doc.source.platform, "chatgpt");
    assert!(!doc.conversations.is_empty());
}
