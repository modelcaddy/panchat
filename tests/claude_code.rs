//! Claude Code's local session history.
//!
//! The fixture is synthetic, and its *shape* was read from 171 real sessions:
//! nodes that are not messages sitting in the parent chain, a compaction that
//! starts a new root, a node written twice, a rewound answer, a subagent
//! transcript, and the side files a store keeps beside its sessions.

use panchat::{ContentPart, Conversation, Document, Method, Role, Severity, WarningCode};

fn store() -> Document {
    let files = panchat::read_path("tests/fixtures/claude_code").unwrap();
    panchat::normalize(&files).unwrap()
}

fn main_session(doc: &Document) -> &Conversation {
    doc.conversations
        .iter()
        .find(|c| c.id == "proj-a/s1")
        .unwrap()
}

#[test]
fn a_session_store_is_recognised_as_a_capture() {
    let doc = store();
    assert_eq!(doc.source.platform, "claude-code");
    assert_eq!(doc.source.variant_version, Some(1));
    assert_eq!(
        doc.source.method,
        Some(Method::Capture),
        "a client's own history, not a vendor export"
    );
}

#[test]
fn a_rewound_answer_is_kept_and_is_off_the_path() {
    let doc = store();
    let c = main_session(&doc);
    let rewound = c.messages.iter().find(|m| m.id == "n5").unwrap();
    assert!(rewound.text().contains("later rewound"));
    assert!(
        !c.active_path.contains(&"n5".to_string()),
        "the thread the user last saw goes through the other answer"
    );
    assert!(c.active_path.contains(&"n6".to_string()));
    assert!(c.off_path_messages().iter().any(|m| m.id == "n5"));
}

#[test]
fn a_local_history_that_records_its_own_graph_does_not_claim_blindness() {
    // SPEC: branches_unavailable is for a method that cannot see alternatives.
    // This one records every rewind, so saying it could not see them would be
    // false — and would train consumers to ignore the warning where it is true.
    let doc = store();
    assert!(!doc
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::BranchesUnavailable));
}

#[test]
fn compaction_does_not_cut_the_thread_in_two() {
    // After a compaction the next node has no parent and a logicalParentUuid.
    // Following the vendor's own link is what keeps the path root-first and
    // whole, from the first message to the last.
    let doc = store();
    let c = main_session(&doc);
    assert_eq!(c.active_path.first().map(String::as_str), Some("n0"));
    assert_eq!(c.active_path.last().map(String::as_str), Some("n10"));
    let boundary = c.messages.iter().find(|m| m.id == "n7").unwrap();
    assert_eq!(boundary.parent.as_deref(), Some("n6"));
}

#[test]
fn nodes_that_are_not_turns_stay_in_the_chain_and_out_of_sight() {
    // Hook output and system events carry a uuid and sit between messages.
    // Dropping them would leave the next message pointing at nothing.
    let doc = store();
    let c = main_session(&doc);
    let ids: Vec<&str> = c.messages.iter().map(|m| m.id.as_str()).collect();
    for m in &c.messages {
        if let Some(p) = &m.parent {
            assert!(ids.contains(&p.as_str()), "{} points at absent {p}", m.id);
        }
    }
    let hook = c.messages.iter().find(|m| m.id == "n0").unwrap();
    assert!(hook.hidden);
    assert!(matches!(hook.content[0], ContentPart::Unknown { .. }));
    let meta = c.messages.iter().find(|m| m.id == "n8").unwrap();
    assert!(
        meta.hidden,
        "an injected summary is not something the user typed"
    );
}

#[test]
fn every_answer_names_its_model() {
    let doc = store();
    let c = main_session(&doc);
    let models: Vec<&str> = c
        .messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .filter_map(|m| m.model.as_deref())
        .collect();
    assert!(models.contains(&"claude-invented-1") && models.contains(&"claude-invented-2"));
    assert!(
        !doc.warnings
            .iter()
            .any(|w| w.code == WarningCode::NoModelIdentity),
        "this source records a model per message, so nothing is missing"
    );
}

#[test]
fn a_node_written_twice_appears_once() {
    // Resuming a session rewrites earlier nodes with new metadata. Ids are
    // unique within a document; the last write wins.
    let doc = store();
    let c = main_session(&doc);
    assert_eq!(c.messages.iter().filter(|m| m.id == "n1").count(), 1);
}

#[test]
fn a_tool_result_names_the_tool_it_answers() {
    let doc = store();
    let c = main_session(&doc);
    let result = c
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .find_map(|p| match p {
            ContentPart::ToolResult { name, .. } => Some(name.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        result.as_deref(),
        Some("Write"),
        "followed by id, not guessed"
    );
}

#[test]
fn the_users_own_title_beats_the_generated_one_and_says_so() {
    let doc = store();
    let c = main_session(&doc);
    assert_eq!(c.title.as_deref(), Some("An invented title the user chose"));
    assert_eq!(c.x["x-panchat"]["claude_code_title_source"], "custom");
}

#[test]
fn the_working_directory_is_the_project() {
    let doc = store();
    assert_eq!(
        main_session(&doc).project.as_ref().map(|p| p.id.as_str()),
        Some("/invented/project")
    );
}

#[test]
fn a_subagent_transcript_is_its_own_conversation_with_its_own_id() {
    // It carries its parent's session id, so the session id alone would collide.
    let doc = store();
    let sub = doc
        .conversations
        .iter()
        .find(|c| c.id == "proj-a/s1/subagents/agent-a1")
        .unwrap();
    assert_eq!(sub.x["x-panchat"]["claude_code_sidechain"], true);
    assert_eq!(sub.active_path.len(), 2, "one leaf, so no reconstruction");
    assert!(!doc
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::BranchPointerBroken));
}

#[test]
fn reasoning_and_inline_images_are_kept_verbatim_and_counted() {
    let doc = store();
    for kind in ["thinking", "image"] {
        let w = doc
            .warnings
            .iter()
            .find(|w| {
                w.code == WarningCode::UnknownContentPart
                    && w.detail.as_deref().unwrap_or_default().contains(kind)
            })
            .unwrap_or_else(|| panic!("{kind} blocks are reported"));
        assert_eq!(w.severity, Severity::Lossy);
    }
}

#[test]
fn what_is_not_read_is_said() {
    let doc = store();
    let details: Vec<&str> = doc
        .warnings
        .iter()
        .filter(|w| w.code == WarningCode::UnhandledExportSection)
        .filter_map(|w| w.detail.as_deref())
        .collect();
    assert!(
        details.iter().any(|d| d.contains("tool-results")),
        "{details:?}"
    );
    assert!(details.iter().any(|d| d.contains("usage")), "{details:?}");
    assert!(
        details.iter().any(|d| d.contains("not sessions")),
        "{details:?}"
    );
}

#[test]
fn memory_files_are_artifacts() {
    let doc = store();
    let memory = doc.artifacts.iter().find(|a| a.kind == "memory").unwrap();
    assert!(memory.text.as_deref().unwrap().contains("Invented memory"));
}

#[test]
fn a_claude_export_is_not_mistaken_for_a_session_store() {
    let files = vec![panchat::ExportFile::new(
        "conversations.json",
        include_str!("fixtures/claude_conversations.json")
            .as_bytes()
            .to_vec(),
    )];
    assert_eq!(panchat::detect(&files).unwrap().platform, "claude");
}
