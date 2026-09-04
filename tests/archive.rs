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

// ---------------------------------------------------------------------------
// Archives that contain archives.
//
// A large account's export arrives as a zip of zips: part archives holding the
// conversations, further part archives holding the attachment bytes. Read one
// level down, it is an export. Read as a flat archive, it is a folder full of
// files nothing recognises, and the user is told their download is junk.
// ---------------------------------------------------------------------------

const SHARD_A: &str = include_str!("fixtures/chatgpt_sharded_000.json");
const SHARD_B: &str = include_str!("fixtures/chatgpt_sharded_001.json");
const MANIFEST: &str = include_str!("fixtures/chatgpt_export_manifest.json");
const ASSET_NAMES: &str = include_str!("fixtures/chatgpt_asset_names.json");

/// The same export twice: split across part archives, and unpacked into one
/// folder. The names follow the pattern reported for 2026 downloads; nothing in
/// the reader keys off them, which is the point of the last assertion here.
fn split_export() -> Vec<u8> {
    let conversations = zip_of(&[
        ("conversations-000.json", SHARD_A.as_bytes()),
        ("conversations-001.json", SHARD_B.as_bytes()),
        ("export_manifest.json", MANIFEST.as_bytes()),
        ("conversation_asset_file_names.json", ASSET_NAMES.as_bytes()),
    ]);
    let assets = zip_of(&[
        ("file-abc123.dat", &vec![7u8; 4096]),
        ("file-doc999.dat", &vec![7u8; 8192]),
    ]);
    zip_of(&[
        ("Conversations__2026-08-20-part-000.zip", &conversations),
        ("Files__2026-08-20-files-000.zip", &assets),
    ])
}

fn split_export_unpacked() -> Vec<ExportFile> {
    vec![
        ExportFile::new("conversations-000.json", SHARD_A.as_bytes().to_vec()),
        ExportFile::new("conversations-001.json", SHARD_B.as_bytes().to_vec()),
        ExportFile::new("export_manifest.json", MANIFEST.as_bytes().to_vec()),
        ExportFile::new(
            "conversation_asset_file_names.json",
            ASSET_NAMES.as_bytes().to_vec(),
        ),
        ExportFile::reference("file-abc123.dat", 4096),
        ExportFile::reference("file-doc999.dat", 8192),
    ]
}

#[test]
fn a_zip_of_part_archives_reads_as_the_export_inside_it() {
    let files = panchat::archive::read_zip_bytes(&split_export()).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();

    assert!(
        paths.contains(&"conversations-000.json") && paths.contains(&"conversations-001.json"),
        "both conversation shards should have come out of the part archive: {paths:?}"
    );
    assert!(
        !paths
            .iter()
            .any(|p| p.ends_with(".zip") || p.contains("part-000")),
        "a part archive is a container, not a file of the export: {paths:?}"
    );
    assert!(
        paths.contains(&"file-abc123.dat"),
        "attachment bytes live in their own part and must still be found: {paths:?}"
    );
}

#[test]
fn a_split_export_and_the_folder_it_unpacks_to_produce_the_same_document() {
    // The property the whole feature exists for. Everything downstream — the
    // adapter, attachment paths, the warnings — was written against the folder.
    let from_zip = panchat::normalize(&[ExportFile::new("export.zip", split_export())]).unwrap();
    let from_folder = panchat::normalize(&split_export_unpacked()).unwrap();

    assert_eq!(
        serde_json::to_value(&from_zip).unwrap(),
        serde_json::to_value(&from_folder).unwrap(),
        "a split export must normalize identically to the folder it unpacks to"
    );
    assert_eq!(from_zip.source.variant_version, Some(2));
    assert!(from_zip.conversations.len() > 1);
}

#[test]
fn attachment_bytes_from_another_part_are_referenced_not_loaded() {
    let files = panchat::archive::read_zip_bytes(&split_export()).unwrap();
    let dat = files.iter().find(|f| f.path == "file-doc999.dat").unwrap();

    assert!(!dat.loaded, "a .dat is an attachment blob, not structure");
    assert!(dat.bytes.is_empty());
    assert_eq!(
        dat.size_bytes, 8192,
        "the real size survives being one archive deeper"
    );
}

#[test]
fn an_ordinary_export_that_contains_a_zip_is_left_alone() {
    // The case that makes an unconditional rule wrong: people upload zips to
    // chatbots, and a 2026 export ships the bytes of what they uploaded. That
    // archive is the user's file. Opening it would spray its contents across
    // the export and could hand an adapter someone's own backup of a
    // conversations.json.
    let uploaded = zip_of(&[(
        "conversations.json",
        b"[{\"mapping\":{},\"title\":\"theirs\"}]",
    )]);
    let archive = zip_of(&[
        ("conversations.json", CHATGPT.as_bytes()),
        ("file-xyz.zip", &uploaded),
    ]);

    let files = panchat::archive::read_zip_bytes(&archive).unwrap();
    let zipped = files.iter().find(|f| f.path == "file-xyz.zip").unwrap();
    assert!(
        !zipped.loaded && zipped.size_bytes > 0,
        "the user's own archive stays a referenced file: {:?}",
        zipped
    );
    assert_eq!(
        files
            .iter()
            .filter(|f| f.path == "conversations.json")
            .count(),
        1,
        "nothing from inside the uploaded archive should appear beside the export"
    );
}

#[test]
fn nesting_stops_after_one_level() {
    // A vendor splitting an export across parts is a real layout. An archive
    // inside one of those parts is not, and following it forever is how a
    // small download costs unbounded work.
    let innermost = zip_of(&[("conversations.json", CHATGPT.as_bytes())]);
    let middle = zip_of(&[("deeper.zip", &innermost)]);
    let outer = zip_of(&[("part-000.zip", &middle)]);

    let files = panchat::archive::read_zip_bytes(&outer).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["deeper.zip"],
        "one level down, and the archive found there is left closed: {paths:?}"
    );

    let err = panchat::normalize(&[ExportFile::new("export.zip", outer)]).unwrap_err();
    assert!(
        err.to_string()
            .contains("nothing inside it looked like an export"),
        "and the user is told what was opened, not given a silent empty result: {err}"
    );
}

#[test]
fn a_part_archives_own_wrapper_folder_is_stripped_too() {
    let part = zip_of(&[("chatgpt-export/conversations.json", CHATGPT.as_bytes())]);
    let outer = zip_of(&[("Conversations__part-000.zip", &part)]);

    let files = panchat::archive::read_zip_bytes(&outer).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths.contains(&"conversations.json"),
        "the wrapper rule has to apply at every level, or an attachment path \
         means something different depending on how deep it was: {paths:?}"
    );
}

#[test]
fn a_part_archive_that_cannot_be_opened_names_itself() {
    let mut corrupt = b"PK\x03\x04".to_vec();
    corrupt.extend_from_slice(b"not actually a zip file");
    let outer = zip_of(&[("Conversations__part-000.zip", &corrupt)]);

    let err = panchat::normalize(&[ExportFile::new("export.zip", outer)]).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains("Conversations__part-000.zip"),
        "with parts inside parts, \"the archive\" is ambiguous: {message}"
    );
    assert!(matches!(err, panchat::Error::Malformed(_)), "{err:?}");
}

// ---------------------------------------------------------------------------
// A folder of archives.
//
// Google Takeout splits a large account across numbered downloads, and what
// people do with several downloads is put them in one folder. Every archive in
// it is a blob as far as the directory walk is concerned, so without help the
// folder reads as a handful of unopenable files.
// ---------------------------------------------------------------------------

const GEMINI: &str = include_str!("fixtures/gemini_myactivity.json");
const SEARCH: &str = include_str!("fixtures/takeout_search_myactivity.json");

/// A directory under the system temp dir, removed when the guard drops.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("panchat-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(name), bytes).unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_folder_of_takeout_archives_reads_as_one_export() {
    let dir = TempDir::new("takeout-split");
    // Takeout wraps everything in one folder and numbers the downloads.
    dir.write(
        "takeout-20260904T090000Z-001.zip",
        &zip_of(&[(
            "Takeout/My Activity/Search/MyActivity.json",
            SEARCH.as_bytes(),
        )]),
    );
    dir.write(
        "takeout-20260904T090000Z-002.zip",
        &zip_of(&[(
            "Takeout/My Activity/Gemini Apps/MyActivity.json",
            GEMINI.as_bytes(),
        )]),
    );

    let files = panchat::read_path(&dir.0).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths
            .iter()
            .any(|p| p.contains("Gemini Apps/MyActivity.json")),
        "the export is inside the downloads, not beside them: {paths:?}"
    );

    let doc = panchat::normalize(&files).unwrap();
    assert_eq!(
        doc.source.platform, "gemini",
        "and the half of the account that is conversations is still found in \
         a download that also holds three other products"
    );
    assert!(!doc.conversations.is_empty());
}

#[test]
fn a_folder_holding_an_unpacked_export_ignores_an_archive_beside_it() {
    // The same protection the nested case has: an export that already yielded
    // its payload needs nothing opened, and the zip in the folder is the user's
    // own file.
    let dir = TempDir::new("takeout-mixed");
    dir.write("conversations.json", CHATGPT.as_bytes());
    dir.write(
        "my-backup.zip",
        &zip_of(&[(
            "conversations.json",
            b"[{\"mapping\":{},\"title\":\"theirs\"}]",
        )]),
    );

    let files = panchat::read_path(&dir.0).unwrap();
    let backup = files.iter().find(|f| f.path == "my-backup.zip").unwrap();
    assert!(
        !backup.loaded,
        "the archive beside an export stays closed: {backup:?}"
    );
    assert_eq!(
        files
            .iter()
            .filter(|f| f.path == "conversations.json")
            .count(),
        1
    );
}
