//! Guards the published JSON Schema against drifting from the Rust types.
//!
//! A prose specification rots within a release; a schema that fails the build
//! cannot. This is the cheap, dependency-free half of that promise: it checks
//! the schema's identity constants and that every field the schema marks
//! required is actually emitted. Full structural validation against a JSON
//! Schema validator lands with the CI harness.

use panchat::{ExportFile, FORMAT_VERSION, SCHEMA_URL};
use serde_json::Value;

const SCHEMA: &str = include_str!("../schema/chat-v0.1.json");
const CHATGPT: &str = include_str!("fixtures/chatgpt_branched.json");

fn schema() -> Value {
    serde_json::from_str(SCHEMA).expect("schema is valid JSON")
}

fn document() -> Value {
    let files = vec![ExportFile::new(
        "conversations.json",
        CHATGPT.as_bytes().to_vec(),
    )];
    let doc = panchat::normalize(&files).unwrap();
    serde_json::to_value(doc).unwrap()
}

#[test]
fn schema_identity_matches_the_crate() {
    let s = schema();
    assert_eq!(
        s["$id"].as_str(),
        Some(SCHEMA_URL),
        "schema $id and ir::SCHEMA_URL must agree"
    );
    assert_eq!(
        s["properties"]["format_version"]["const"].as_str(),
        Some(FORMAT_VERSION),
        "schema format_version and ir::FORMAT_VERSION must agree"
    );
}

fn assert_required(value: &Value, required: &Value, what: &str) {
    for key in required.as_array().unwrap_or(&Vec::new()) {
        let key = key.as_str().unwrap();
        assert!(
            value.get(key).is_some(),
            "{what} is missing schema-required field `{key}`"
        );
    }
}

#[test]
fn emitted_document_carries_every_required_field() {
    let s = schema();
    let doc = document();

    assert_required(&doc, &s["required"], "document");
    assert_required(&doc["source"], &s["$defs"]["source"]["required"], "source");

    let conversation = &doc["conversations"][0];
    assert_required(
        conversation,
        &s["$defs"]["conversation"]["required"],
        "conversation",
    );

    for message in conversation["messages"].as_array().unwrap() {
        assert_required(message, &s["$defs"]["message"]["required"], "message");
    }
    for warning in doc["warnings"].as_array().unwrap() {
        assert_required(warning, &s["$defs"]["warning"]["required"], "warning");
    }
}

/// Rule 3: consumers must preserve fields they do not recognise. A document
/// carrying a namespaced third-party key must survive a round trip through the
/// typed representation with that key intact.
#[test]
fn namespaced_extension_keys_survive_a_round_trip() {
    let mut doc = panchat::normalize(&[ExportFile::new(
        "conversations.json",
        CHATGPT.as_bytes().to_vec(),
    )])
    .unwrap();
    doc.x.insert(
        "x-modelcaddy".into(),
        serde_json::json!({ "space": "ascii", "ref_num": 12 }),
    );
    doc.conversations[0]
        .x
        .insert("x-vendor".into(), serde_json::json!("kept"));

    let json = serde_json::to_string(&doc).unwrap();
    let back: panchat::Document = serde_json::from_str(&json).unwrap();

    assert_eq!(back.x["x-modelcaddy"]["ref_num"], 12);
    assert_eq!(back.conversations[0].x["x-vendor"], "kept");
}

/// Rule 4: no app-specific concepts in the core namespace.
#[test]
fn core_namespace_is_vendor_neutral() {
    let doc = document();
    let serialized = serde_json::to_string(&doc).unwrap();
    for forbidden in ["modelcaddy", "ref_num", "space_id", "digest"] {
        let in_core = doc
            .as_object()
            .unwrap()
            .keys()
            .any(|k| k.contains(forbidden));
        assert!(
            !in_core,
            "`{forbidden}` must not appear as a top-level core key"
        );
    }
    assert!(serialized.contains("format_version"));
}

/// SPEC.md: a producer MUST NOT emit a `parent` naming an id absent from
/// `messages`. The ChatGPT export's root node carries no message, so the naive
/// mapping produces exactly that dangling pointer.
#[test]
fn no_producer_emits_a_dangling_parent() {
    for fixture in [
        (
            "conversations.json",
            include_str!("fixtures/chatgpt_branched.json"),
        ),
        (
            "conversations.json",
            include_str!("fixtures/claude_conversations.json"),
        ),
        (
            "MyActivity.json",
            include_str!("fixtures/gemini_myactivity.json"),
        ),
    ] {
        let doc = panchat::normalize(&[ExportFile::new(fixture.0, fixture.1.as_bytes().to_vec())])
            .unwrap();

        for c in &doc.conversations {
            let ids: Vec<&str> = c.messages.iter().map(|m| m.id.as_str()).collect();
            for m in &c.messages {
                if let Some(parent) = &m.parent {
                    assert!(
                        ids.contains(&parent.as_str()),
                        "message {} points at absent parent {parent}",
                        m.id
                    );
                }
            }
            // SPEC.md: active_path MUST contain only ids present in messages.
            for id in &c.active_path {
                assert!(
                    ids.contains(&id.as_str()),
                    "active_path names absent id {id}"
                );
            }
        }
    }
}
