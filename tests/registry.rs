//! An adapter written outside this crate.
//!
//! Everything here uses only the public API, the way a third party would. The
//! property under test is that such an adapter is *selected* — before the
//! registry opened, one compiled cleanly and was never asked.

use panchat::{
    Adapter, ContentPart, Conversation, Detection, Document, Error, ExportFile, Message, Registry,
    Role, Source, Warnings,
};
use serde_json::Value;

/// A made-up in-house logging format: `{"house_log": [{"who": ..., "said": ...}]}`.
///
/// The platform name is configured rather than compiled in, which is the case
/// that needed `Detection` to own its strings.
struct HouseLog {
    platform: String,
}

impl HouseLog {
    fn new(platform: &str) -> Self {
        Self {
            platform: platform.to_string(),
        }
    }

    fn log<'a>(&self, files: &'a [ExportFile]) -> Option<(&'a ExportFile, Value)> {
        files.iter().find_map(|f| {
            let v: Value = serde_json::from_slice(&f.bytes).ok()?;
            v.get("house_log")?.as_array()?;
            Some((f, v))
        })
    }
}

impl Adapter for HouseLog {
    fn platform(&self) -> &str {
        &self.platform
    }

    fn variant(&self) -> &str {
        "house_log_v1"
    }

    fn detect(&self, files: &[ExportFile]) -> Option<Detection> {
        self.log(files)?;
        Some(Detection {
            platform: self.platform.clone().into(),
            variant: "house_log_v1".into(),
            variant_version: 1,
            confidence: 0.9,
            notes: Vec::new(),
        })
    }

    fn parse(&self, files: &[ExportFile], _warnings: &mut Warnings) -> Result<Document, Error> {
        let (_, v) = self
            .log(files)
            .ok_or_else(|| Error::Malformed("no house log".into()))?;
        let mut doc = Document::new(Source::new(self.platform.clone(), "house_log_v1"));
        let mut c = Conversation::new("house-1");
        for (i, row) in v["house_log"].as_array().unwrap().iter().enumerate() {
            let mut m = Message::new(format!("m{i}"), Role::parse(row["who"].as_str().unwrap()));
            m.content.push(ContentPart::Text {
                text: row["said"].as_str().unwrap().to_string(),
            });
            c.messages.push(m);
        }
        c.active_path = c.messages.iter().map(|m| m.id.clone()).collect();
        doc.conversations.push(c);
        Ok(doc)
    }
}

const HOUSE: &str = r#"{"house_log": [
    {"who": "user", "said": "An invented question."},
    {"who": "assistant", "said": "An invented answer."}
]}"#;

const CHATGPT: &str = include_str!("fixtures/chatgpt_branched.json");

fn one(name: &str, body: &str) -> Vec<ExportFile> {
    vec![ExportFile::new(name, body.as_bytes().to_vec())]
}

#[test]
fn the_builtin_registry_does_not_know_the_format() {
    assert!(panchat::detect(&one("log.json", HOUSE)).is_none());
    assert!(panchat::normalize(&one("log.json", HOUSE)).is_err());
}

#[test]
fn a_registered_adapter_is_selected_by_the_same_detection() {
    let registry = Registry::builtin().with(HouseLog::new("acme-internal"));
    let doc = registry.normalize(&one("log.json", HOUSE)).unwrap();

    assert_eq!(
        doc.source.platform, "acme-internal",
        "the configured name, not a compiled-in one"
    );
    assert_eq!(doc.source.variant.as_deref(), Some("house_log_v1"));
    assert_eq!(doc.source.variant_version, Some(1));
    assert_eq!(doc.conversations[0].messages.len(), 2);
}

#[test]
fn a_registered_adapter_does_not_take_a_builtin_vendors_files() {
    // Adding an adapter must not change what happens to every other export.
    let registry = Registry::builtin().with(HouseLog::new("acme-internal"));
    let doc = registry
        .normalize(&one("conversations.json", CHATGPT))
        .unwrap();
    assert_eq!(doc.source.platform, "chatgpt");
}

#[test]
fn an_empty_registry_recognises_nothing_and_says_so() {
    let err = Registry::empty()
        .normalize(&one("conversations.json", CHATGPT))
        .unwrap_err();
    assert!(matches!(err, Error::NotRecognized(_)), "{err:?}");
}

#[test]
fn a_registry_lists_what_it_can_read() {
    let registry = Registry::builtin().with(HouseLog::new("acme-internal"));
    assert_eq!(
        registry.platforms(),
        vec![
            "chatgpt",
            "claude",
            "claude-code",
            "gemini",
            "acme-internal"
        ]
    );
}
