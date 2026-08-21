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

fn claude_files() -> Vec<ExportFile> {
    files(&[
        ("conversations.json", CLAUDE_CONVS),
        ("projects.json", CLAUDE_PROJECTS),
        ("memories.json", CLAUDE_MEMORIES),
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
