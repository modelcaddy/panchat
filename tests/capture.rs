//! A document built from a capture rather than an export.
//!
//! SPEC.md requires a capture producer to warn `branches_unavailable`, because
//! a branch-free conversation means something different under each method.
//! Before this file, nothing in the crate produced a capture, so nothing
//! checked the rule — and the JSON Schema had quietly lost both the `method`
//! field and the warning code, which is what an unexercised rule does.

use panchat::capture::Capture;
use panchat::{ContentPart, Conversation, Document, Message, Method, Role, Severity, WarningCode};

fn message(id: &str, role: Role, text: &str) -> Message {
    let mut m = Message::new(id, role);
    m.content.push(ContentPart::Text { text: text.into() });
    m
}

fn rendered(id: &str) -> Conversation {
    let mut c = Conversation::new(id);
    c.messages = vec![
        message("a", Role::User, "An invented question."),
        message("b", Role::Assistant, "An invented answer."),
        message("c", Role::User, "An invented follow-up."),
    ];
    c
}

fn captured() -> Document {
    let mut capture = Capture::new("gemini", "web_capture_v1");
    capture.push(rendered("one"));
    capture.push(rendered("two"));
    capture.finish()
}

#[test]
fn a_capture_says_it_is_one() {
    let doc = captured();
    assert_eq!(doc.source.method, Some(Method::Capture));
    let json = serde_json::to_value(&doc).unwrap();
    assert_eq!(json["source"]["method"], "capture");
}

#[test]
fn a_capture_cannot_be_finished_without_branches_unavailable() {
    let doc = captured();
    let w = doc
        .warnings
        .iter()
        .find(|w| w.code == WarningCode::BranchesUnavailable)
        .expect("the rule holds by construction, not by memory");
    assert_eq!(
        w.severity,
        Severity::Lossy,
        "the branches may well have existed; they could not be seen"
    );
    assert_eq!(w.count, 2, "one folded warning covering every conversation");
}

#[test]
fn rendering_order_becomes_the_thread() {
    let doc = captured();
    let c = &doc.conversations[0];
    let order: Vec<&str> = c.active_messages().iter().map(|m| m.id.as_str()).collect();
    assert_eq!(order, vec!["a", "b", "c"]);
    assert_eq!(c.messages[0].parent, None);
    assert_eq!(c.messages[1].parent.as_deref(), Some("a"));
    assert_eq!(c.messages[2].parent.as_deref(), Some("b"));
}

#[test]
fn a_graph_the_client_stored_itself_is_left_alone() {
    // A client's local history can carry its own parent pointers. It knows
    // better than rendering order does.
    let mut c = rendered("stored");
    c.messages[2].parent = Some("a".into());
    let mut capture = Capture::new("some-client", "local_history_v1");
    capture.push(c);
    let doc = capture.finish();
    assert_eq!(
        doc.conversations[0].messages[2].parent.as_deref(),
        Some("a")
    );
}

#[test]
fn an_empty_capture_still_says_what_it_could_not_see() {
    let doc = Capture::new("gemini", "web_capture_v1").finish();
    assert!(doc
        .warnings
        .iter()
        .any(|w| w.code == WarningCode::BranchesUnavailable));
}
