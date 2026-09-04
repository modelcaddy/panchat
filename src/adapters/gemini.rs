//! Google Takeout — "My Activity" for Gemini Apps.
//!
//! This is the export that most tests what the representation is for, because
//! it is not a conversation export at all. Google does not hand over chats; it
//! hands over an **activity log**, the same log that records a search or a
//! video watched, filtered to the Gemini product. One record is one exchange:
//! the prompt in `title` behind a localized prefix, the answer as HTML in
//! `safeHtmlItem`, and a timestamp. There is no conversation object, no id, no
//! model, and no thread.
//!
//! So the losses here are structural rather than incidental, and every one of
//! them is reported:
//!
//! - **Nothing groups the turns.** Google gives a `titleUrl` on some records
//!   and nothing at all on others, so a multi-turn chat arrives as unrelated
//!   rows. Where the vendor supplies that pointer it is used; where it does
//!   not, each record is its own conversation. Stitching rows together on a
//!   time gap — which several tools in the wild do, on a thirty-minute
//!   threshold — would be inventing a conversation the export does not contain,
//!   and this crate does not invent.
//! - **There are no ids.** They are synthesized, deterministically from the
//!   timestamp and title, so that re-importing a fresh export of the same
//!   account de-duplicates instead of doubling.
//! - **The answer is HTML.** It is kept exactly as Google wrote it. Turning it
//!   into Markdown would be a reformat, and the specification forbids one:
//!   what the vendor stored is what the consumer gets.
//! - **Not every record is a conversation.** Canvas creation, image generation,
//!   draft selection and feedback all live in the same file. They are counted
//!   and reported rather than parsed into empty chats.
//!
//! **This shape has not been observed here.** It is reconstructed from the
//! source of some twenty independent parsers that read real exports, which
//! agree on the keys and disagree about almost everything else. See
//! `docs/formats/gemini.md`, which records who was read and what remains
//! unconfirmed.

use super::{Adapter, Detection, ExportFile};
use crate::ir::{ContentPart, Conversation, Document, Message, Role, Source};
use crate::warning::{Severity, WarningCode, Warnings};
use crate::Error;
use serde_json::Value;
use std::collections::BTreeMap;

pub struct Gemini;

const PLATFORM: &str = "gemini";
/// Takeout's activity log, filtered to Gemini Apps. Numbered from 1 like every
/// other vendor's series here; the number is ours, not Google's.
const VARIANT_V1: &str = "takeout_myactivity_v1";

/// What Google puts in front of the user's own words in `title`.
///
/// The list is localized, and it is not documented anywhere — these are the
/// prefixes parsers have been seen stripping in the wild. A record whose prefix
/// is not on this list is **not** discarded and its title is **not** guessed
/// at: the title is taken whole, which is at worst a few extra words of the
/// user's own language in front of their own question, and at best correct.
const PROMPT_PREFIXES: &[&str] = &[
    "Prompted ",
    "Asked ",
    "Said ",
    "Submitted query ",
    // Japanese
    "送信したメッセージ: ",
    // Greek
    "Υποβλήθηκε το ερώτημα ",
    // Spanish
    "Has dicho: ",
    "Hiciste la petición ",
];

/// Products whose activity this adapter claims. `header` and `products` are
/// translated per locale — "Gemini アプリ", "Εφαρμογές Gemini" — but the product
/// name itself survives translation, which is what makes a substring test the
/// robust one.
const PRODUCT_MARKERS: &[&str] = &["gemini", "bard"];

impl Adapter for Gemini {
    fn platform(&self) -> &'static str {
        PLATFORM
    }

    fn variant(&self) -> &'static str {
        VARIANT_V1
    }

    fn detect(&self, files: &[ExportFile]) -> Option<Detection> {
        let found = activity_files(files);
        if found.is_empty() {
            return None;
        }

        let total: usize = found.iter().map(|(_, r)| r.len()).sum();
        let conversational = found
            .iter()
            .flat_map(|(_, r)| r.iter())
            .filter(|r| is_conversational(r))
            .count();
        // A file of Gemini activity with nothing conversational in it is still
        // this adapter's file — saying so beats handing the user "unrecognised".
        let confidence = if conversational > 0 { 0.95 } else { 0.75 };

        let mut notes = Vec::new();
        if found.len() > 1 {
            notes.push(format!("{} activity file(s)", found.len()));
        }
        notes.push(format!("{total} activity record(s)"));
        if conversational < total {
            notes.push(format!(
                "{} of them carry an exchange; the rest are other Gemini activity",
                conversational
            ));
        }
        Some(Detection {
            platform: PLATFORM,
            variant: VARIANT_V1,
            variant_version: 1,
            confidence,
            notes,
        })
    }

    fn parse(&self, files: &[ExportFile], warnings: &mut Warnings) -> Result<Document, Error> {
        let found = activity_files(files);
        if found.is_empty() {
            return Err(Error::Malformed(
                "no Gemini Apps activity file in this export".into(),
            ));
        }
        let paths: Vec<&str> = found.iter().map(|(p, _)| p.as_str()).collect();
        let records: Vec<&Value> = found.iter().flat_map(|(_, r)| r.iter()).collect();

        let mut doc = Document::new(Source::new(PLATFORM, VARIANT_V1).with_variant_version(1));

        // Per-message model identity and branch alternatives never existed in
        // an activity log, so these are `info`: nothing was lost, there was
        // nothing there.
        warnings.note(WarningCode::NoModelIdentity, Severity::Info);

        // Grouped by the vendor's own pointer where there is one, and by the
        // record itself where there is not. Ordered, so a document does not
        // depend on the order Google happened to write its log in — which is
        // newest first, the reverse of a transcript.
        let mut grouped: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
        let mut standalone: Vec<&Value> = Vec::new();
        let mut skipped = 0usize;
        let mut skipped_kinds: Vec<String> = Vec::new();

        for record in records {
            if !is_conversational(record) {
                skipped += 1;
                let kind = activity_kind(record);
                if !skipped_kinds.contains(&kind) {
                    skipped_kinds.push(kind);
                }
                continue;
            }
            match conversation_pointer(record) {
                Some(id) => grouped.entry(id).or_default().push(record),
                None => standalone.push(record),
            }
        }

        if skipped > 0 {
            skipped_kinds.sort();
            let mut w = crate::warning::Warning::new(
                WarningCode::UnhandledExportSection,
                // The records exist and are not represented. They are not
                // conversations, but calling that `info` would be pretending
                // the export held nothing else.
                Severity::Lossy,
            )
            .with_detail(format!(
                "{skipped} Gemini activity record(s) are not exchanges and were not read: {}",
                skipped_kinds.join(", ")
            ));
            w.count = skipped as u32;
            warnings.push(w);
        }

        let mut synthesized = 0usize;
        for (pointer, mut records) in grouped {
            sort_by_time(&mut records);
            let conversation = build(&pointer, &records, warnings);
            doc.conversations.push(conversation);
        }
        for record in standalone {
            let id = synthesized_id(record);
            synthesized += 1;
            let conversation = build(&id, &[record], warnings);
            doc.conversations.push(conversation);
        }
        sort_conversations(&mut doc.conversations);

        if synthesized > 0 {
            let mut w = crate::warning::Warning::new(WarningCode::SynthesizedId, Severity::Info)
                .with_detail(
                    "Takeout records no conversation id; ids are derived from the timestamp and \
                     title so a re-export of the same account de-duplicates",
                );
            w.count = synthesized as u32;
            warnings.push(w);
        }

        doc.x.insert(
            "x-panchat".into(),
            serde_json::json!({ "gemini_activity_files": paths }),
        );
        Ok(doc)
    }
}

/// Every Gemini slice of a Takeout activity log, parsed, in path order.
///
/// Found by shape, never by name: the file is `MyActivity.json` in English and
/// something else entirely in Greek, and every other Google product in the same
/// download writes a file of that name too. What identifies it is an array of
/// activity records naming Gemini as the product.
///
/// **Every** matching file, not the first. Takeout splits a large account
/// across numbered downloads, and this crate already carries the scar of an
/// adapter that read one file and reported the 100 conversations it found out
/// of the 1,285 that were there.
fn activity_files(files: &[ExportFile]) -> Vec<(String, Vec<Value>)> {
    let mut out: Vec<(String, Vec<Value>)> = files
        .iter()
        .filter(|f| f.loaded && f.lower_path().ends_with(".json"))
        .filter_map(|f| {
            let Ok(Value::Array(records)) = serde_json::from_slice::<Value>(&f.bytes) else {
                return None;
            };
            // Sampled rather than scanned. `detect` runs for every adapter on
            // every input, and an activity log can hold hundreds of thousands
            // of rows.
            let sample = &records[..records.len().min(20)];
            if sample.is_empty() || !sample.iter().all(is_activity_record) {
                return None;
            }
            if !sample.iter().any(names_gemini) {
                return None;
            }
            Some((f.path.clone(), records))
        })
        .collect();
    // Sorted, so a document does not depend on the order a filesystem or an
    // archive happened to hand the files over.
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A row of a Google activity log, whatever product it belongs to.
fn is_activity_record(v: &Value) -> bool {
    let Some(o) = v.as_object() else {
        return false;
    };
    o.contains_key("title") && (o.contains_key("time") || o.contains_key("header"))
}

/// Whether this row belongs to Gemini rather than to Search, YouTube, or any
/// other product sharing the same download and the same filename.
///
/// Claiming another product's file is the one detection mistake here with a
/// real cost, so the test is narrow on purpose.
fn names_gemini(v: &Value) -> bool {
    let header = v.get("header").and_then(Value::as_str).unwrap_or_default();
    let products = v
        .get("products")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    // Matched only where Google *names the product*. Deliberately not matched
    // anywhere in `titleUrl` as text: a Search record for the word "gemini"
    // carries it in the query string, and a rule that reads the whole URL turns
    // somebody's search history into their chat history.
    let named = format!("{header} {products}").to_lowercase();
    if PRODUCT_MARKERS.iter().any(|m| named.contains(m)) {
        return true;
    }
    // A link to Gemini's own host is the other honest marker, and the one that
    // survives a locale this adapter cannot read.
    v.get("titleUrl")
        .and_then(Value::as_str)
        .is_some_and(on_gemini_host)
}

/// Whether a URL's host is Gemini's own, rather than a URL that merely mentions
/// it. Compared on the host alone, so `google.com/search?q=gemini` does not
/// pass and `https://gemini.google.com/app/c/x` does.
fn on_gemini_host(url: &str) -> bool {
    let lower = url.to_lowercase();
    let after_scheme = lower.split_once("://").map(|(_, r)| r).unwrap_or(&lower);
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    // A userinfo segment can put anything in front of the real host.
    let host = host.rsplit('@').next().unwrap_or(host);
    host == "gemini.google.com" || host == "bard.google.com"
}

/// Whether this record is an exchange, rather than one of the other things
/// Gemini logs: a canvas created, an image generated, a draft preferred,
/// feedback given.
///
/// An answer is the strongest evidence, and it survives translation where a
/// title prefix does not. A recognised prefix is the fallback, for the record
/// whose answer Google did not keep.
fn is_conversational(v: &Value) -> bool {
    !response_html(v).is_empty() || strip_prompt_prefix(title(v)).is_some()
}

/// A label for a record this adapter does not read, for the warning that says
/// so. The vendor's own words, truncated at the first space — "Created", "Used"
/// — because the rest is the name of somebody's document.
fn activity_kind(v: &Value) -> String {
    let title = title(v);
    match title.split_whitespace().next() {
        Some(first) if !first.is_empty() => first.to_string(),
        _ => "untitled".to_string(),
    }
}

fn title(v: &Value) -> &str {
    v.get("title").and_then(Value::as_str).unwrap_or_default()
}

/// The user's words, with Google's prefix removed — or `None` when no known
/// prefix is there.
fn strip_prompt_prefix(title: &str) -> Option<&str> {
    PROMPT_PREFIXES
        .iter()
        .find_map(|p| title.strip_prefix(p))
        .map(str::trim)
}

/// Every `safeHtmlItem[].html`, joined.
///
/// Joined, not indexed: a long answer arrives as several items, and the parsers
/// that take `[0]` silently lose the rest of it.
fn response_html(v: &Value) -> String {
    v.get("safeHtmlItem")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.get("html").and_then(Value::as_str))
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}

/// Google's own conversation pointer, where a record carries one.
///
/// `titleUrl` is a `gemini.google.com` link whose last path segment identifies
/// the chat. Query and fragment are cut so the same chat does not become two.
fn conversation_pointer(v: &Value) -> Option<String> {
    let url = v.get("titleUrl").and_then(Value::as_str)?;
    if !on_gemini_host(url) {
        return None;
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let last = path.trim_end_matches('/').rsplit('/').next()?;
    match last.is_empty() || last.eq_ignore_ascii_case("app") {
        true => None,
        false => Some(last.to_string()),
    }
}

/// A stable id for a record the export gave no id.
///
/// Derived from the timestamp and title rather than from position, because
/// Takeout prepends new activity: a fresh export of the same account shifts
/// every index and would re-import as a duplicate of everything.
fn synthesized_id(v: &Value) -> String {
    let seed = format!("{}|{}", time_string(v).unwrap_or_default(), title(v));
    format!("gemini-{:016x}", fnv1a(seed.as_bytes()))
}

/// FNV-1a, written out rather than taken from the standard library, because
/// `DefaultHasher` is explicitly allowed to change between Rust releases and
/// these ids have to survive one.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn time_string(v: &Value) -> Option<&str> {
    v.get("time").and_then(Value::as_str)
}

/// Takeout writes newest first; a transcript reads the other way.
fn sort_by_time(records: &mut [&Value]) {
    records.sort_by(|a, b| {
        time_string(a)
            .unwrap_or_default()
            .cmp(time_string(b).unwrap_or_default())
    });
}

fn sort_conversations(conversations: &mut [Conversation]) {
    conversations.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// Timestamps are ISO 8601 with a `Z`. Normalized to RFC 3339 so every adapter
/// emits one shape, and kept verbatim when it will not parse rather than
/// dropped.
fn iso(v: &Value) -> Option<String> {
    let s = time_string(v)?;
    match chrono::DateTime::parse_from_rfc3339(s) {
        Ok(dt) => Some(dt.to_rfc3339()),
        Err(_) => Some(s.to_string()),
    }
}

fn build(id: &str, records: &[&Value], warnings: &mut Warnings) -> Conversation {
    let mut conversation = Conversation::new(id);
    conversation.created_at = records.first().and_then(|r| iso(r));
    conversation.updated_at = records.last().and_then(|r| iso(r));
    if conversation.created_at.is_none() {
        warnings.note_for(WarningCode::MissingTimestamps, Severity::Lossy, id);
    }
    // Google titles nothing. Synthesizing one from the first prompt is the
    // obvious thing to do and is forbidden for good reason: a title the user
    // never wrote, presented as theirs, is a small lie that survives every
    // later copy.
    conversation.url = records
        .first()
        .and_then(|r| r.get("titleUrl"))
        .and_then(Value::as_str)
        .map(str::to_string);
    conversation.raw = Some(Value::Array(records.iter().map(|r| (*r).clone()).collect()));

    for (index, record) in records.iter().enumerate() {
        let created = iso(record);

        let prompt = strip_prompt_prefix(title(record)).unwrap_or_else(|| title(record).trim());
        let attachments = attachments(record, warnings, id);
        if !prompt.is_empty() || !attachments.is_empty() {
            let mut message = Message::new(format!("{id}-r{index}-user"), Role::User);
            message.created_at.clone_from(&created);
            if !prompt.is_empty() {
                message.content.push(ContentPart::Text {
                    text: prompt.to_string(),
                });
            }
            message.content.extend(attachments);
            conversation.messages.push(message);
        }

        let html = response_html(record);
        if !html.is_empty() {
            let mut message = Message::new(format!("{id}-r{index}-model"), Role::Assistant);
            // The two halves of one exchange share a timestamp, because the
            // export records the exchange rather than either turn.
            message.created_at = created;
            message.parent = conversation.messages.last().map(|m| m.id.clone());
            // Kept as Google wrote it. The answer is HTML; converting it to
            // Markdown would be a reformat, and a producer must not reformat
            // what a vendor stored.
            message.content.push(ContentPart::Text { text: html });
            conversation.messages.push(message);
        }
    }

    // Parent pointers run through the whole conversation so a consumer that
    // walks the graph gets the same order as one that walks the path.
    let mut previous: Option<String> = None;
    for message in &mut conversation.messages {
        if message.parent.is_none() {
            message.parent.clone_from(&previous);
        }
        previous = Some(message.id.clone());
    }
    // An activity log cannot branch, so the active path is every message —
    // populated rather than left empty, so a consumer needs one code path.
    conversation.active_path = conversation.messages.iter().map(|m| m.id.clone()).collect();
    conversation
}

/// Files the user attached, under whichever of the three key names this export
/// uses. Their bytes are never in the activity log.
fn attachments(record: &Value, warnings: &mut Warnings, conversation: &str) -> Vec<ContentPart> {
    let mut out = Vec::new();
    let mut push = |name: Option<&str>| {
        out.push(ContentPart::Attachment {
            id: None,
            name: name.map(str::to_string),
            mime_type: None,
            // Takeout's activity log references what was attached and ships
            // none of it.
            path: None,
            size_bytes: None,
        });
    };

    for key in ["attachedFiles", "attachmentInfo"] {
        if let Some(items) = record.get(key).and_then(Value::as_array) {
            for item in items {
                push(item.get("name").and_then(Value::as_str));
            }
        }
    }
    if let Some(image) = record.get("imageFile").and_then(Value::as_str) {
        push(Some(image));
    }

    for _ in 0..out.len() {
        warnings.note_for(
            WarningCode::AttachmentNotIncluded,
            Severity::Lossy,
            conversation,
        );
    }
    out
}
