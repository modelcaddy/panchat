//! Fixture tests.
//!
//! These are the only thing standing between the crate and silent rot when a
//! vendor changes an export shape, so they assert on the properties that make
//! the crate worth using — branch preservation, lossiness reporting, and
//! round-tripping unknown data — not merely that parsing succeeded.

use panchat::warning::{Severity, WarningCode};
use panchat::{ContentPart, ExportFile, Role};

fn files(pairs: &[(&str, &str)]) -> Vec<ExportFile> {
    pairs
        .iter()
        .map(|(name, body)| ExportFile::new(*name, body.as_bytes().to_vec()))
        .collect()
}

const CHATGPT: &str = include_str!("fixtures/chatgpt_branched.json");
const CLAUDE_CONVS: &str = include_str!("fixtures/claude_conversations.json");
const CLAUDE_PROJECTS: &str = include_str!("fixtures/claude_projects.json");
const CLAUDE_MEMORIES: &str = include_str!("fixtures/claude_memories.json");

fn chatgpt_files() -> Vec<ExportFile> {
    files(&[("conversations.json", CHATGPT)])
}

const CHATGPT_SHARD_A: &str = include_str!("fixtures/chatgpt_sharded_000.json");
const CHATGPT_SHARD_B: &str = include_str!("fixtures/chatgpt_sharded_001.json");
const CHATGPT_MANIFEST: &str = include_str!("fixtures/chatgpt_export_manifest.json");
const CHATGPT_ASSET_NAMES: &str = include_str!("fixtures/chatgpt_asset_names.json");

/// A large export as ChatGPT ships one now: conversations split across
/// numbered shards, a manifest naming them, an asset-name map, and the
/// attachment bytes sitting alongside — referenced, not loaded, exactly as
/// `read_path` hands them to an adapter.
fn chatgpt_sharded_files() -> Vec<ExportFile> {
    let mut out = files(&[
        ("conversations-000.json", CHATGPT_SHARD_A),
        ("conversations-001.json", CHATGPT_SHARD_B),
        ("export_manifest.json", CHATGPT_MANIFEST),
        ("conversation_asset_file_names.json", CHATGPT_ASSET_NAMES),
    ]);
    out.push(ExportFile::reference("file-abc123.dat", 4096));
    out.push(ExportFile::reference("file-doc999.dat", 6272133));
    out
}

fn claude_files() -> Vec<ExportFile> {
    files(&[
        ("conversations.json", CLAUDE_CONVS),
        ("projects.json", CLAUDE_PROJECTS),
        ("memories.json", CLAUDE_MEMORIES),
    ])
}

const CLAUDE_PROJECT_DIR: &str = include_str!("fixtures/claude_project_dir.json");
const CLAUDE_MEMORIES_V2: &str = include_str!("fixtures/claude_memories_v2.json");
const CLAUDE_DESIGN_CHAT: &str = include_str!("fixtures/claude_design_chat.json");

/// A Claude export as it ships now: side-cars in directories, a second chat
/// format in `design_chats/`, and a `memories.json` that is one object rather
/// than a list of memory rows.
fn claude_directory_files() -> Vec<ExportFile> {
    files(&[
        ("conversations.json", CLAUDE_CONVS),
        ("projects/proj-9.json", CLAUDE_PROJECT_DIR),
        ("design_chats/design-1.json", CLAUDE_DESIGN_CHAT),
        ("memories.json", CLAUDE_MEMORIES_V2),
        ("users.json", r#"[{"uuid":"acct-1","full_name":"George"}]"#),
        ("login_history.json", r#"{"login_events":[]}"#),
    ])
}

#[test]
fn detects_vendors_without_being_told() {
    let d = panchat::detect(&chatgpt_files()).expect("chatgpt detected");
    assert_eq!(d.platform, "chatgpt");
    assert!(d.confidence > 0.9);

    let d = panchat::detect(&claude_files()).expect("claude detected");
    assert_eq!(d.platform, "claude");
    assert!(d.confidence > 0.9);
}

/// A user who renamed their export, or unpacked it into an odd layout, must not
/// be told their data is unrecognisable. Detection falls back to shape.
#[test]
fn detects_renamed_exports_by_shape() {
    let renamed = files(&[("my-chatgpt-backup-2026.json", CHATGPT)]);
    let d = panchat::detect(&renamed).expect("chatgpt detected by shape, not filename");
    assert_eq!(d.platform, "chatgpt");
    assert_eq!(panchat::normalize(&renamed).unwrap().conversations.len(), 1);

    let renamed = files(&[("claude-backup.json", CLAUDE_CONVS)]);
    let d = panchat::detect(&renamed).expect("claude detected by shape, not filename");
    assert_eq!(d.platform, "claude");
    assert_eq!(panchat::normalize(&renamed).unwrap().conversations.len(), 1);
}

#[test]
fn unrecognised_input_is_an_error_not_a_guess() {
    let junk = files(&[("notes.json", r#"{"hello":"world"}"#)]);
    assert!(panchat::detect(&junk).is_none());
    assert!(panchat::normalize(&junk).is_err());
}

/// The property the whole crate exists for: a regenerated answer is a sibling
/// branch, and flattening to the active path would delete it.
#[test]
fn chatgpt_keeps_off_path_branches() {
    let doc = panchat::normalize(&chatgpt_files()).unwrap();
    let c = &doc.conversations[0];

    // 5 nodes carry messages; the `root` node has none.
    assert_eq!(c.messages.len(), 5);

    // The active branch skips n2, the regenerated-away answer.
    assert_eq!(c.active_path, vec!["n0", "n1", "n3", "n4"]);

    let off: Vec<&str> = c
        .off_path_messages()
        .iter()
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(off, vec!["n2"], "the discarded regeneration must survive");

    let n2 = c.messages.iter().find(|m| m.id == "n2").unwrap();
    assert_eq!(n2.text(), "First answer, later regenerated.");
}

#[test]
fn chatgpt_records_per_message_model_and_parent_edges() {
    let doc = panchat::normalize(&chatgpt_files()).unwrap();
    let c = &doc.conversations[0];

    let n2 = c.messages.iter().find(|m| m.id == "n2").unwrap();
    let n3 = c.messages.iter().find(|m| m.id == "n3").unwrap();
    assert_eq!(n2.model.as_deref(), Some("gpt-4o"));
    assert_eq!(n3.model.as_deref(), Some("gpt-5.2"));
    // Both regenerations hang off the same user turn.
    assert_eq!(n2.parent.as_deref(), Some("n1"));
    assert_eq!(n3.parent.as_deref(), Some("n1"));
}

/// System framing is marked, not deleted — the existing app-side importer
/// drops it, which makes the transcript unreproducible.
#[test]
fn chatgpt_keeps_hidden_system_turns() {
    let doc = panchat::normalize(&chatgpt_files()).unwrap();
    let c = &doc.conversations[0];
    let n0 = c.messages.iter().find(|m| m.id == "n0").unwrap();
    assert_eq!(n0.role, Role::System);
    assert!(n0.hidden);
    assert_eq!(n0.text(), "You are helpful.");
}

#[test]
fn chatgpt_reports_referenced_but_missing_attachment() {
    let doc = panchat::normalize(&chatgpt_files()).unwrap();
    let c = &doc.conversations[0];
    let n4 = c.messages.iter().find(|m| m.id == "n4").unwrap();

    assert!(matches!(
        n4.content.as_slice(),
        [ContentPart::Text { .. }, ContentPart::Attachment { .. }]
    ));
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::AttachmentNotIncluded
            && w.message_id.as_deref() == Some("n4")));
}

#[test]
fn claude_parses_conversations_projects_and_memories() {
    let doc = panchat::normalize(&claude_files()).unwrap();
    assert_eq!(doc.conversations.len(), 1);

    let c = &doc.conversations[0];
    assert_eq!(c.title.as_deref(), Some("Vault design"));
    assert_eq!(
        c.project.as_ref().unwrap().name.as_deref(),
        Some("ModelCaddy")
    );
    assert_eq!(c.messages[0].role, Role::User);
    // A flat export has one branch, and it is the active one.
    assert_eq!(c.active_path, vec!["m-1", "m-2"]);

    let kinds: Vec<&str> = doc.artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.contains(&"project"));
    assert!(kinds.contains(&"memory"));
}

/// An unmodelled block is carried through whole rather than replaced with a
/// placeholder — the rule that lets an out-of-date parser degrade instead of
/// destroying data.
#[test]
fn unknown_content_is_preserved_verbatim_and_reported() {
    let doc = panchat::normalize(&claude_files()).unwrap();
    let m2 = &doc.conversations[0].messages[1];

    let unknown = m2
        .content
        .iter()
        .find_map(|p| match p {
            ContentPart::Unknown { kind, raw } => Some((kind.clone(), raw.clone())),
            _ => None,
        })
        .expect("unmodelled block kept");
    assert_eq!(unknown.0.as_deref(), Some("citation_block"));
    assert_eq!(
        unknown.1.get("citation").and_then(|v| v.as_str()),
        Some("something new we do not model"),
        "the original payload must survive intact"
    );

    assert!(doc
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::UnknownContentPart));
}

/// Absence in the source is Info, not Lossy: nothing was lost, it never existed.
#[test]
fn claude_reports_that_the_format_has_no_model_identity() {
    let doc = panchat::normalize(&claude_files()).unwrap();
    let w = doc
        .warnings
        .iter()
        .find(|w| w.code == WarningCode::NoModelIdentity)
        .expect("format-level gap reported");
    assert_eq!(w.severity, Severity::Info);
}

#[test]
fn document_round_trips_through_json() {
    let doc = panchat::normalize(&chatgpt_files()).unwrap();
    let json = panchat::export::to_json(&doc).unwrap();
    let back: panchat::Document = serde_json::from_str(&json).unwrap();

    assert_eq!(back.format_version, panchat::FORMAT_VERSION);
    assert_eq!(back.conversations.len(), doc.conversations.len());
    assert_eq!(
        back.conversations[0].active_path,
        doc.conversations[0].active_path
    );
    assert_eq!(back.conversations[0].messages.len(), 5);
    assert_eq!(back.warnings.len(), doc.warnings.len());
}

#[test]
fn markdown_renders_active_branch_by_default_and_marks_off_path() {
    use panchat::export::{to_markdown, Branches};
    let doc = panchat::normalize(&chatgpt_files()).unwrap();
    let c = &doc.conversations[0];

    let active = to_markdown(c, Branches::ActiveOnly);
    assert!(active.starts_with("---\n"));
    assert!(active.contains("title: Auth refactor"));
    assert!(active.contains("Second answer, the one kept."));
    assert!(!active.contains("First answer, later regenerated."));

    let all = to_markdown(c, Branches::All);
    assert!(all.contains("First answer, later regenerated."));
    assert!(all.contains("(off-path branch)"));
}

#[test]
fn turns_jsonl_emits_one_line_per_message() {
    use panchat::export::{to_turns_jsonl, Branches};
    let doc = panchat::normalize(&claude_files()).unwrap();
    let out = to_turns_jsonl(&doc, Branches::ActiveOnly).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("\"role\":\"user\""));
    assert!(lines[1].contains("\"role\":\"assistant\""));
}

/// The failure this shape was built to cause: an importer that reads the first
/// file it recognises keeps one shard and reports no error at all.
#[test]
fn chatgpt_sharded_export_reads_every_shard() {
    let files = chatgpt_sharded_files();
    let d = panchat::detect(&files).expect("sharded chatgpt detected");
    assert_eq!(d.platform, "chatgpt");
    assert_eq!(
        d.variant, "official_export_v2",
        "a consumer must be able to tell one file did not hold everything"
    );
    assert_eq!(d.variant_version, 2);

    let doc = panchat::normalize(&files).unwrap();
    let ids: Vec<&str> = doc.conversations.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["sh-1", "sh-2"], "shards read in manifest order");
    assert_eq!(doc.source.variant.as_deref(), Some("official_export_v2"));
    assert_eq!(doc.source.variant_version, Some(2));
}

/// A shard the manifest promised but the download does not contain is a hole
/// in the data, not a smaller export.
#[test]
fn chatgpt_reports_a_shard_the_manifest_promised() {
    let doc = panchat::normalize(&chatgpt_sharded_files()).unwrap();
    let w = doc
        .warnings
        .iter()
        .find(|w| w.code == WarningCode::UnhandledExportSection)
        .expect("missing shard reported");
    assert_eq!(w.severity, Severity::Dropped);
    assert!(w
        .detail
        .as_deref()
        .unwrap_or_default()
        .contains("conversations-404.json"));
}

/// These exports ship the attachment bytes. Reporting them missing anyway —
/// or losing the name the user uploaded them under — is the lie this adapter
/// exists to avoid.
#[test]
fn chatgpt_resolves_attachments_whose_bytes_shipped() {
    let doc = panchat::normalize(&chatgpt_sharded_files()).unwrap();
    let c = &doc.conversations[0];
    let n0 = c.messages.iter().find(|m| m.id == "n0").unwrap();

    let attachments: Vec<(&Option<String>, &Option<String>, &Option<String>)> = n0
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Attachment { id, name, path, .. } => Some((id, name, path)),
            _ => None,
        })
        .collect();
    assert_eq!(
        attachments.len(),
        2,
        "the image is listed as a part and in metadata; it is one file"
    );

    let image = attachments
        .iter()
        .find(|(id, ..)| id.as_deref() == Some("file-service://file-abc123"))
        .expect("image kept its vendor pointer");
    assert_eq!(image.1.as_deref(), Some("diagram.png"));
    assert_eq!(image.2.as_deref(), Some("file-abc123.dat"));

    // The upload that never appears in the content parts at all.
    let csv = attachments
        .iter()
        .find(|(id, ..)| id.as_deref() == Some("file-doc999"))
        .expect("metadata-only upload recorded");
    assert_eq!(csv.1.as_deref(), Some("products.csv"));
    assert_eq!(csv.2.as_deref(), Some("file-doc999.dat"));

    assert!(
        !doc.warnings
            .iter()
            .any(|w| w.code == WarningCode::AttachmentNotIncluded
                && w.message_id.as_deref() == Some("n0")),
        "bytes that shipped must not be reported as missing"
    );

    // Voice assets expire and are not in the export; that one is still lossy.
    let n2 = c.messages.iter().find(|m| m.id == "n2").unwrap();
    assert!(matches!(
        n2.content.as_slice(),
        [ContentPart::Attachment { path: None, .. }]
    ));
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::AttachmentNotIncluded
            && w.message_id.as_deref() == Some("n2")));
}

/// A voice turn's transcript is the turn. Left unmodelled it reads as an empty
/// message with an audio blob attached.
#[test]
fn chatgpt_keeps_voice_transcripts_as_text() {
    let doc = panchat::normalize(&chatgpt_sharded_files()).unwrap();
    let n1 = doc.conversations[0]
        .messages
        .iter()
        .find(|m| m.id == "n1")
        .unwrap();
    assert_eq!(n1.text(), "The diagram shows the token rotation flow.");
}

/// Reasoning is preserved whole rather than folded into the answer: a summary
/// of the model's thinking is not what the assistant said.
#[test]
fn chatgpt_keeps_reasoning_turns_verbatim() {
    let doc = panchat::normalize(&chatgpt_sharded_files()).unwrap();
    let m1 = doc.conversations[1]
        .messages
        .iter()
        .find(|m| m.id == "m1")
        .unwrap();

    let (kind, raw) = m1
        .content
        .iter()
        .find_map(|p| match p {
            ContentPart::Unknown { kind, raw } => Some((kind.clone(), raw.clone())),
            _ => None,
        })
        .expect("reasoning kept");
    assert_eq!(kind.as_deref(), Some("thoughts"));
    assert!(raw.get("thoughts").is_some(), "the payload survives intact");
    assert_eq!(m1.text(), "", "reasoning is not the assistant's answer");
}
/// The newer layout keeps `conversations.json` and moves everything else. An
/// adapter written for the flat shape reads it without complaint and returns
/// no projects, no memories, and none of the design chats.
#[test]
fn claude_directory_export_reads_the_side_car_directories() {
    let files = claude_directory_files();
    let d = panchat::detect(&files).expect("claude detected");
    assert_eq!(d.variant, "official_export_v2");
    assert_eq!(d.variant_version, 2);

    let doc = panchat::normalize(&files).unwrap();
    let kinds: Vec<&str> = doc.artifacts.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.contains(&"project"), "projects/<uuid>.json read");
    assert!(
        kinds.contains(&"project_doc"),
        "a project's documents are the part the user wrote"
    );

    // The memory shape with no row ids at all — parsed as the old one it
    // yields nothing, silently.
    let memories: Vec<&str> = doc
        .artifacts
        .iter()
        .filter(|a| a.kind == "memory")
        .filter_map(|a| a.title.as_deref())
        .collect();
    assert_eq!(
        memories,
        vec!["conversations memory", "/areas/podium.md", "/profile.md"]
    );

    // Account metadata is skipped on purpose, and said out loud.
    let skipped: Vec<&str> = doc
        .warnings
        .iter()
        .filter(|w| w.code == WarningCode::UnhandledExportSection)
        .filter_map(|w| w.detail.as_deref())
        .collect();
    assert!(skipped.contains(&"users.json"));
    assert!(skipped.contains(&"login_history.json"));
}

/// Design chats are conversations in a different dialect. Ignoring them loses
/// whole chats, not fields.
#[test]
fn claude_reads_design_chats_as_conversations() {
    let doc = panchat::normalize(&claude_directory_files()).unwrap();
    let chat = doc
        .conversations
        .iter()
        .find(|c| c.id == "design-1")
        .expect("design chat read");

    assert_eq!(
        chat.project.as_ref().unwrap().name.as_deref(),
        Some("Politis Hub")
    );
    assert_eq!(chat.active_path, vec!["d0", "d1"]);
    assert_eq!(
        chat.x
            .get("x-panchat")
            .and_then(|v| v.get("claude_export_section")),
        Some(&serde_json::json!("design_chats")),
        "which half of the export a conversation came from cannot be inferred"
    );

    let user = &chat.messages[0];
    assert_eq!(user.role, Role::User);
    // Prompt text, then the attachment's own text, which shipped inline.
    assert!(user.text().contains("Redo the landing page."));
    assert!(user.text().contains("Use the brand palette."));
    let named: Vec<&str> = user
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Attachment { name, .. } => name.as_deref(),
            _ => None,
        })
        .collect();
    assert_eq!(named, vec!["Design System", "screenshot.png"]);
    // Only the one with no inline content is actually missing.
    assert_eq!(
        doc.warnings
            .iter()
            .filter(|w| w.code == WarningCode::AttachmentNotIncluded
                && w.message_id.as_deref() == Some("d0"))
            .count(),
        1
    );

    let assistant = &chat.messages[1];
    assert_eq!(
        assistant.text(),
        "I'll start by looking at the current state of things."
    );
    assert!(assistant.content.iter().any(
        |p| matches!(p, ContentPart::ToolUse { name, .. } if name.as_deref() == Some("list_files"))
    ));
    assert!(assistant
        .content
        .iter()
        .any(|p| matches!(p, ContentPart::ToolResult { .. })));
    // An empty thinking placeholder is not a thought that was lost.
    let unknown: Vec<&str> = assistant
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Unknown { kind, .. } => kind.as_deref(),
            _ => None,
        })
        .collect();
    assert_eq!(unknown, vec!["error"]);
}

/// Vendors ship exports as zip archives and people pass them along unopened.
/// "Unrecognised export" is the wrong answer to the commonest mistake.
///
/// What the right answer is depends on the `zip` feature: without it, say to
/// unpack the archive; with it, the archive is read, so this input fails as the
/// corrupt zip it actually is. Either way it is named as an archive.
#[test]
fn a_zip_archive_is_named_rather_than_called_unrecognisable() {
    let mut zip = b"PK\x03\x04".to_vec();
    zip.extend_from_slice(&[0u8; 32]);
    let files = vec![ExportFile::new("claude-export.zip", zip)];

    let err = panchat::normalize(&files).unwrap_err().to_string();
    assert!(err.contains("zip archive"), "{err}");
    assert!(err.contains("claude-export.zip"), "{err}");
    #[cfg(not(feature = "zip"))]
    assert!(err.contains("unpack"), "{err}");
}

/// Both layouts stay readable. A vendor changing its export must not turn an
/// older download in someone's Downloads folder into an unreadable file.
#[test]
fn chatgpt_old_single_file_layout_still_reads() {
    let d = panchat::detect(&chatgpt_files()).expect("flat chatgpt detected");
    assert_eq!(d.variant, "official_export_v1");
    assert_eq!(d.variant_version, 1);

    let doc = panchat::normalize(&chatgpt_files()).unwrap();
    assert_eq!(doc.conversations.len(), 1);
    assert_eq!(doc.source.variant.as_deref(), Some("official_export_v1"));
    assert_eq!(doc.source.variant_version, Some(1));
}

/// The layout before `conversation_asset_file_names.json` existed: the bytes
/// keep their original name and the asset id is only a prefix of it.
#[test]
fn chatgpt_resolves_attachments_in_the_older_filename_layout() {
    let mut files = files(&[("conversations.json", CHATGPT)]);
    files.push(ExportFile::reference(
        "dalle-generations/abc-a-cat-on-a-roof.webp",
        9000,
    ));

    let doc = panchat::normalize(&files).unwrap();
    let n4 = doc.conversations[0]
        .messages
        .iter()
        .find(|m| m.id == "n4")
        .unwrap();
    let (name, path) = n4
        .content
        .iter()
        .find_map(|p| match p {
            ContentPart::Attachment { name, path, .. } => Some((name.clone(), path.clone())),
            _ => None,
        })
        .expect("attachment part");
    assert_eq!(
        path.as_deref(),
        Some("dalle-generations/abc-a-cat-on-a-roof.webp")
    );
    assert_eq!(name.as_deref(), Some("abc-a-cat-on-a-roof.webp"));
    assert!(
        !doc.warnings
            .iter()
            .any(|w| w.code == WarningCode::AttachmentNotIncluded),
        "bytes that shipped under the old naming must not be reported missing"
    );
}

#[test]
fn claude_old_flat_layout_still_reads() {
    let d = panchat::detect(&claude_files()).expect("flat claude detected");
    assert_eq!(d.variant, "official_export_v1");
    assert_eq!(d.variant_version, 1);

    let doc = panchat::normalize(&claude_files()).unwrap();
    assert_eq!(doc.conversations.len(), 1);
    // The older memory rows, each with their own uuid.
    assert!(doc.artifacts.iter().any(|a| a.kind == "memory"));
    assert!(doc.artifacts.iter().any(|a| a.kind == "project"));
}

/// An export that has both — the flat side-cars and the new directories —
/// must not make the adapter choose one.
#[test]
fn claude_reads_a_mixed_layout_whole() {
    let mut mixed = claude_files();
    mixed.extend(files(&[
        ("projects/proj-9.json", CLAUDE_PROJECT_DIR),
        ("design_chats/design-1.json", CLAUDE_DESIGN_CHAT),
    ]));

    let doc = panchat::normalize(&mixed).unwrap();
    assert_eq!(
        doc.conversations.len(),
        2,
        "flat conversation plus design chat"
    );
    let projects: Vec<&str> = doc
        .artifacts
        .iter()
        .filter(|a| a.kind == "project")
        .filter_map(|a| a.title.as_deref())
        .collect();
    assert_eq!(projects, vec!["ModelCaddy", "Politis Hub"]);
}

/// A document has to say which generation of the vendor's export made it. A
/// third party reading the JSON cannot ask us afterwards, and the answer
/// changes what the data means — v1 ChatGPT rarely ships attachment bytes,
/// v2 does; v1 Claude has no design chats, v2 does.
#[test]
fn every_document_states_the_export_shape_it_came_from() {
    let cases: [(Vec<ExportFile>, &str, u32); 4] = [
        (chatgpt_files(), "chatgpt", 1),
        (chatgpt_sharded_files(), "chatgpt", 2),
        (claude_files(), "claude", 1),
        (claude_directory_files(), "claude", 2),
    ];

    for (files, platform, version) in cases {
        let doc = panchat::normalize(&files).unwrap();
        assert_eq!(doc.source.platform, platform);
        assert_eq!(
            doc.source.variant_version,
            Some(version),
            "{platform} v{version} must say so in the document"
        );
        assert_eq!(
            doc.source.variant.as_deref(),
            Some(format!("official_export_v{version}").as_str())
        );

        // And it survives serialization, which is the only form a third party
        // ever sees.
        let json = panchat::export::to_json(&doc).unwrap();
        let back: panchat::Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.source.variant_version, Some(version));
    }
}

/// The newer layout without the parts that make it obvious: no shards, no
/// design chats — just the files that only the current export ships.
#[test]
fn export_shape_is_recognised_without_the_obvious_markers() {
    let mut chatgpt = files(&[("conversations.json", CHATGPT)]);
    chatgpt.push(ExportFile::new(
        "conversation_asset_file_names.json",
        CHATGPT_ASSET_NAMES.as_bytes().to_vec(),
    ));
    let d = panchat::detect(&chatgpt).expect("chatgpt detected");
    assert_eq!(d.variant_version, 2, "one shard, but the v2 asset map");

    let claude = files(&[
        ("conversations.json", CLAUDE_CONVS),
        ("memories.json", CLAUDE_MEMORIES_V2),
    ]);
    let d = panchat::detect(&claude).expect("claude detected");
    assert_eq!(d.variant_version, 2, "no directories, but the v2 memories");
}

// ---------------------------------------------------------------------------
// Gemini — Google Takeout's activity log.
//
// The export that most tests what this representation is for, because it is not
// a conversation export. Google hands over the same activity log that records a
// search, filtered to one product: no conversation object, no id, no model, no
// thread. What matters here is that every one of those absences is reported
// rather than papered over, and that nothing is invented to fill them.
// ---------------------------------------------------------------------------

const GEMINI: &str = include_str!("fixtures/gemini_myactivity.json");
const TAKEOUT_SEARCH: &str = include_str!("fixtures/takeout_search_myactivity.json");

fn gemini_files() -> Vec<ExportFile> {
    // The real path is localized — Greek exports name neither directory in
    // English — so the name here is deliberately not the one Google writes.
    files(&[("My Activity/Gemini Apps/renamed-by-the-user.json", GEMINI)])
}

#[test]
fn gemini_is_detected_by_shape_not_by_filename() {
    let d = panchat::detect(&gemini_files()).expect("gemini activity detected");
    assert_eq!(d.platform, "gemini");
    assert_eq!(d.variant, "takeout_myactivity_v1");
    assert_eq!(d.variant_version, 1);
}

#[test]
fn gemini_does_not_claim_another_products_activity_log() {
    // Every Google product writes `MyActivity.json` into the same download,
    // with the same record shape. Claiming one of those would turn somebody's
    // search history into a chat transcript.
    let other = files(&[("My Activity/Search/MyActivity.json", TAKEOUT_SEARCH)]);
    assert!(
        panchat::detect(&other).is_none(),
        "an activity log for another product is not this adapter's file"
    );
}

#[test]
fn gemini_groups_only_on_the_pointer_google_supplies() {
    let doc = panchat::normalize(&gemini_files()).unwrap();

    let threaded = doc
        .conversations
        .iter()
        .find(|c| c.id == "invented-thread-1")
        .expect("records sharing a titleUrl belong to one conversation");
    assert_eq!(
        threaded.messages.len(),
        4,
        "two exchanges, each a prompt and an answer"
    );
    assert_eq!(
        threaded.messages[0].text(),
        "Explain the first half of the made-up thing",
        "and in the order they were said, not the order Takeout writes them, \
         which is newest first"
    );

    // Everything else stands alone. Stitching rows together on a time gap is
    // what several tools in the wild do, and it invents a conversation the
    // export does not contain.
    assert_eq!(
        doc.conversations.len(),
        3,
        "one threaded conversation, and the two records with no pointer left \
         standing alone"
    );
}

#[test]
fn gemini_keeps_the_whole_answer_and_keeps_it_as_html() {
    let doc = panchat::normalize(&gemini_files()).unwrap();
    let threaded = doc
        .conversations
        .iter()
        .find(|c| c.id == "invented-thread-1")
        .unwrap();
    let answer = threaded.messages[1].text();

    assert!(
        answer.contains("The first half does not exist.")
            && answer.contains("Nor does this list item."),
        "a long answer arrives as several html items and all of them are the \
         answer; taking the first is the commonest way to lose half of one: {answer}"
    );
    assert!(
        answer.contains("<p>") && answer.contains("<ul>"),
        "the vendor stored HTML, so HTML is what a consumer gets — converting \
         it to Markdown would be a reformat, and producers must not reformat: {answer}"
    );
}

#[test]
fn gemini_reads_a_localized_record() {
    let doc = panchat::normalize(&gemini_files()).unwrap();
    let japanese = doc
        .conversations
        .iter()
        .find(|c| c.messages.iter().any(|m| m.text().contains("作り話")))
        .expect("a Japanese record is still a record");

    assert_eq!(
        japanese.messages[0].text(),
        "これは作り話です",
        "the prefix Google puts in front of the user's words is translated too"
    );
}

#[test]
fn gemini_says_what_it_did_not_read() {
    let doc = panchat::normalize(&gemini_files()).unwrap();

    let skipped = doc
        .warnings
        .iter()
        .find(|w| w.code == WarningCode::UnhandledExportSection)
        .expect("canvas and draft-selection records are activity, and are not exchanges");
    assert_eq!(skipped.severity, Severity::Lossy, "they were in the export");
    assert_eq!(skipped.count, 2);
    let detail = skipped.detail.as_deref().unwrap_or_default();
    assert!(
        detail.contains("Created") && detail.contains("Selected"),
        "and the user is told which kinds, in the vendor's own words: {detail}"
    );

    assert!(
        doc.warnings
            .iter()
            .any(|w| w.code == WarningCode::NoModelIdentity && w.severity == Severity::Info),
        "an activity log never recorded a model, so nothing was lost — info, not lossy"
    );
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.code == WarningCode::SynthesizedId),
        "there are no ids in this format and the document must admit it"
    );
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.code == WarningCode::AttachmentNotIncluded && w.severity == Severity::Lossy),
        "the file was attached and its bytes are not in the download"
    );
}

#[test]
fn gemini_synthesized_ids_survive_a_re_export() {
    // Takeout prepends new activity, so every index shifts when an account
    // exports again. An id derived from position would re-import the whole
    // history as new; one derived from the record's own content does not.
    let first = panchat::normalize(&gemini_files()).unwrap();

    let mut records: Vec<serde_json::Value> = serde_json::from_str(GEMINI).unwrap();
    records.insert(
        0,
        serde_json::json!({
            "header": "Gemini Apps",
            "title": "Prompted Something asked after the first export",
            "time": "2026-08-01T10:00:00.000Z",
            "products": ["Gemini Apps"],
            "safeHtmlItem": [{ "html": "<p>An invented later answer.</p>" }]
        }),
    );
    let later = panchat::normalize(&files(&[(
        "MyActivity.json",
        &serde_json::to_string(&records).unwrap(),
    )]))
    .unwrap();

    for old in &first.conversations {
        assert!(
            later.conversations.iter().any(|c| c.id == old.id),
            "{} changed id between two exports of the same account",
            old.id
        );
    }
    assert_eq!(later.conversations.len(), first.conversations.len() + 1);
}

#[test]
fn gemini_never_invents_a_title() {
    let doc = panchat::normalize(&gemini_files()).unwrap();
    assert!(
        doc.conversations.iter().all(|c| c.title.is_none()),
        "Takeout titles nothing, and a title the user never wrote is a small \
         lie that survives every later copy"
    );
}

#[test]
fn gemini_keeps_the_record_it_could_not_fully_represent() {
    let doc = panchat::normalize(&gemini_files()).unwrap();
    let raw = doc
        .conversations
        .iter()
        .filter_map(|c| c.raw.as_ref())
        .count();
    assert_eq!(
        raw,
        doc.conversations.len(),
        "every conversation carries the vendor's own rows, so anything this \
         version failed to model is still there"
    );
}

#[test]
fn gemini_populates_an_active_path_it_does_not_need() {
    // An activity log cannot branch. The path is filled in anyway so a consumer
    // needs one code path rather than an empty-case branch per vendor.
    let doc = panchat::normalize(&gemini_files()).unwrap();
    for c in &doc.conversations {
        assert_eq!(
            c.active_path.len(),
            c.messages.len(),
            "{} left a consumer to guess the order",
            c.id
        );
        assert!(c.off_path_messages().is_empty());
    }
}

#[test]
fn takeout_exported_as_html_says_which_mistake_was_made() {
    // The format is chosen before Takeout builds the download, HTML is the
    // default, and finding out it is unreadable means asking Google again and
    // waiting. "Unrecognised export" is a cruel way to deliver that.
    let html = files(&[(
        "Takeout/My Activity/Gemini Apps/MyActivity.html",
        "<html><body>an activity rendering</body></html>",
    )]);
    let err = panchat::normalize(&html).unwrap_err();
    let message = err.to_string();

    assert!(
        message.contains("JSON") && message.contains("takeout.google.com"),
        "the error has to say what to do instead: {message}"
    );
}
