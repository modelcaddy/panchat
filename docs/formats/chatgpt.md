# ChatGPT export log

What OpenAI's data export has actually looked like, each time one was read, and what the adapter
does about it. OpenAI publishes no schema and announces no changes, so this file is the only record
of when a shape moved — and the fixture tests in `tests/` are the only thing that catches it moving
again.

Entries are newest first. Each records what was observed, not what the vendor claims.

Each shape gets a number, and a document produced from it says which one it was:
`source.variant_version` in the emitted JSON, `variant` for the same thing in words. The numbering is
ours, not OpenAI's.

| Shape | Emitted as | First seen | Mark |
|---|---|---|---|
| v2 | `official_export_v2` | 2026-08-20 | `export_manifest.json` or `conversation_asset_file_names.json`, usually sharded |
| v1 | `official_export_v1` | before 2026 | one `conversations.json`, no manifest |

Sharding alone is not the test: a small account gets one `conversations.json` in the v2 layout.

---

## v2 · 2026-08-20 — sharded export, attachment bytes included

**Observed:** a 619 MB unpacked export, 1,285 conversations, from an account of ordinary size.

**Layout**

```text
export_manifest.json                  version 1
conversations-000.json … -012.json    100 conversations each, 85 in the last
conversation_asset_file_names.json    463 entries: "file-<id>.dat" → original filename
chat.html                             53 MB, the same conversations rendered
ads.json                              {}
file-<id>.dat                         attachment bytes, 651 files
file_<hex>.dat                        voice and video assets
```

`export_manifest.json` is authoritative and names the shards in order:

```json
"logical_files": {
  "conversations.json": { "files": ["conversations-000.json", "…"], "sharded": true }
}
```

**What changed from the older shape**

| | Older | This export |
|---|---|---|
| Conversations | one `conversations.json` | 13 numbered shards plus a manifest |
| Attachment bytes | often absent; named `<id>-<original name>.<ext>`, DALL·E images under `dalle-generations/` | present as `<id>.dat`, names in a side-car map |
| Asset pointer schemes | `file-service://` | `file-service://` (80) and `sediment://` (545) |
| Conversation keys | `gizmo_id`, `moderation_results`, `safe_urls` | `conversation_template_id` carries the `g-p-…` project id; new `memory_scope`, `is_starred`, `is_study_mode`, `is_do_not_remember`, `voice`, `pinned_time` |

**Content types seen**

| `content_type` | Count | Handling |
|---|---|---|
| `text` | 13,115 | text parts |
| `thoughts` | 1,665 | preserved verbatim as `unknown` — reasoning is not the answer, and merging it would put words in the assistant's mouth |
| `multimodal_text` | 1,044 | parts, per part type below |
| `reasoning_recap` | 786 | preserved verbatim as `unknown`; the payload is UI chrome ("Thought for 26 seconds") |

**Part types inside `multimodal_text`**

| Part | Count | Handling |
|---|---|---|
| `audio_transcription` | 766 | text — the transcript *is* the turn |
| `image_asset_pointer` | 323 | attachment, resolved to `<id>.dat` when present |
| `real_time_user_audio_video_asset_pointer` | 303 | attachments from the pointers nested inside it |
| `audio_asset_pointer` | 302 | attachment |

`metadata.attachments` (380 entries) lists files the user uploaded, with a real `mime_type` and
size. They appear nowhere in the content parts — a turn that attached a 6 MB CSV otherwise reads as
a conversation about nothing.

**Result of reading it**

- Emitted as `variant: "official_export_v2"`, `variant_version: 2`.
- 1,285 conversations, 13,948 text parts, 987 attachments (464 with bytes present), 2,451 `unknown`
  parts (`thoughts` and `reasoning_recap`).
- 523 attachments are genuinely absent — expired `sediment://` voice assets, mostly.
- Peak memory 288 MB, because attachment blobs are referenced rather than read.

**What it cost before the adapter changed:** 100 conversations imported, 1,185 lost, no warning. The
filename heuristic matched `conversation_asset_file_names.json` first and nothing read past a single
file.

**Still not modelled**

- `metadata.content_references` (7,249), `search_result_groups` (720), `image_results` (197),
  `code_blocks` (309) — kept in `Conversation.raw`, not surfaced as parts.
- `chat.html` and `ads.json` are skipped deliberately, each reported `unhandled_export_section`.

---

## v1 · before 2026 — single-file export

**Layout:** `conversations.json`, `user.json`, `message_feedback.json`, `model_comparisons.json`,
`shared_conversations.json`, `chat.html`, and a `dalle-generations/` directory of generated images.

Attachments, when present at all, kept their original name with the asset id as a prefix:
`file-abc123-diagram.png`. There was no name map, so the id is a prefix rather than the whole
filename — the adapter resolves both layouts, and the old one is pinned by
`chatgpt_resolves_attachments_in_the_older_filename_layout` in `tests/adapters.rs`.

Read as variant `official_export_v1`, `variant_version: 1`. Still supported; a download sitting in
someone's Downloads folder must not become unreadable because the vendor moved on.
