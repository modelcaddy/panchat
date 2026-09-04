# panchat

Read AI chat exports from any vendor into one representation — and say out loud what each export
left behind.

> **Status: pre-1.0.** The interchange format is v0.1 and the shape may still move.

```rust,no_run
let files = panchat::read_path("chatgpt-export/")?;
let doc = panchat::normalize(&files)?;   // vendor detected automatically

for conversation in &doc.conversations {
    println!("{}", conversation.title.as_deref().unwrap_or("(untitled)"));
    for message in conversation.active_messages() {
        println!("  {}: {}", message.role.as_str(), message.text());
    }
}

for w in &doc.warnings {
    eprintln!("{:?}: {}", w.severity, w.code.describe());
}
# Ok::<(), panchat::Error>(())
```

Or without writing any code:

```text
panchat ~/Downloads/chatgpt-export --inspect
panchat ~/Downloads/chatgpt-export --format markdown -o out.md
panchat ~/Downloads/chatgpt-export --format json | jq '.conversations | length'
```

## What it does

- **Zero-config detection.** You pass files; it works out the vendor.
- **Keeps the branch graph.** A ChatGPT export is a node graph, not a list: every regeneration and
  every edited prompt is a sibling branch. Flattening to the active path — what most importers do —
  destroys them irreversibly. This crate keeps every message and records which branch was current.
- **Says what was lost.** Every parse returns structured warnings: missing timestamps, attachments
  referenced but not included, unrecognised content parts, broken branch pointers.
- **Never drops what it does not understand.** Unknown message parts are preserved verbatim, the
  original vendor payload is kept on each conversation, and every struct has a namespaced extension
  point. An out-of-date parser degrades; it does not destroy.

## Supported

| Platform | Export | Status |
|---|---|---|
| ChatGPT | `conversations.json`, or sharded `conversations-000.json` … plus `export_manifest.json` | Conversations, branch graph, per-message model, voice transcripts, attachments (resolved to the bytes the export ships) |
| Claude | `conversations.json` + `projects.json` / `projects/`, `memories.json`, `design_chats/` | Conversations, Claude Design chats, projects and their documents, memories, tool calls, attachments (referenced) |

## Input

The zip the vendor gave you, an unpacked directory, or a single `conversations.json` — all three
read the same. The CLI reads archives out of the box; as a library it is the `zip` feature, off by
default so the dependency is yours to opt into:

```toml
panchat = { version = "0.2", features = ["zip"] }
```

An archive is recognised by its contents rather than its name, and the vendor's wrapper folder is
stripped, so a zip and the directory it unpacks to produce byte-identical documents. Without the
feature, an archive is still named rather than called unrecognisable:

```text
error: unrecognised export: claude-export.zip is a zip archive; unpack it and pass the folder,
or build with the `zip` feature to read archives directly
```

Validation is detection: `panchat::detect` returns the vendor and a confidence, `normalize` returns
`Err(NotRecognized)` with the files it saw when nothing matches, and everything it *did* recognise
but could not fully represent comes back as warnings on the document rather than as an error.

Directories and archives are walked whole. Structured files are read; attachment blobs are recorded
by name and size without being loaded, so a 619 MB export costs megabytes, not gigabytes.

## Format changes

Vendors change their exports without saying so, and the change alters what the data means: one
generation of a ChatGPT export ships attachment bytes and the one before it does not. So every
document says which generation it came from —

```json
"source": { "platform": "chatgpt", "variant": "official_export_v2", "variant_version": 2 }
```

— and a consumer asks `variant_version >= 2` rather than matching strings. What each one looked like
when it was read, and what that cost, is logged per source — [ChatGPT](docs/formats/chatgpt.md),
[Claude](docs/formats/claude.md) — and every change to this crate is in [CHANGELOG.md](CHANGELOG.md).
Both old and new layouts stay readable: a download that has been sitting in someone's Downloads
folder for a year must not become unreadable because the vendor moved on.

## Known lossiness, by vendor

The point of this table is that no vendor publishes it.

| | ChatGPT | Claude |
|---|---|---|
| Branch / regeneration history | present, preserved | not in export |
| Per-message model identity | present | **absent from the format** |
| Attachment bytes | shipped for uploads and generated images, referenced only for expired voice assets | not included, referenced only |
| Tool / code-interpreter payloads | partially typed, rest preserved verbatim | typed (`tool_use` / `tool_result`) |
| Reasoning turns | present as `thoughts` / `reasoning_recap`, kept verbatim, never merged into the answer | not in export |
| Timestamps | float unix seconds | ISO 8601 |
| Project membership | via template/gizmo id, name not included | full, with name |
| Side-cars | none in the export | projects and their docs, memories; account and login history skipped on purpose |

## Output formats

| Format | Use |
|---|---|
| `json` | The full representation |
| `jsonl` | One conversation per line |
| `turns` | One `{role, content}` per line — for eval and fine-tuning pipelines. Lossy by construction. |
| `markdown` | Human- and git-readable, YAML frontmatter |

Writing a vendor's own export format back out is not supported and will not be: no vendor imports
its own export.

## The format

[`SPEC.md`](SPEC.md) specifies the shape for third parties implementing a producer or consumer —
every property, the branch-graph rules, warning codes, and conformance requirements.
[`schema/chat-v0.1.json`](schema/chat-v0.1.json) is the machine-readable half.

Every platform produces the **same document**. Only `source.platform` differs; the keys, the nesting
and the schema do not. A consumer written against one vendor's output already reads them all, and a
vendor's own concepts live under a namespaced `x-` key rather than changing the shape.

## Design rules

1. A tiny required core — only what every vendor actually has.
2. Extension points reserved from v0.1, because they cannot be retrofitted.
3. Consumers must preserve fields they do not recognise.
4. Nothing app-specific in the core namespace.

## Contributing

Two vendors is a tool; enough vendors is infrastructure — and nobody has an account on every
platform or a copy of every export shape. Adapters arrive as pull requests, and
[CONTRIBUTING.md](CONTRIBUTING.md) walks through writing one: the trait is four methods, and the
review bar is about honesty rather than polish — declare what you dropped, and never drop what you
did not understand.
What is planned, and in what order, is in [ROADMAP.md](ROADMAP.md); every item there is an open
issue.

You do not need to write Rust to help. **"My export stopped reading"** is the report we cannot get
any other way, because no vendor announces a change. Please do not attach your export to anything:
it contains every word you have ever typed into that product, and the issue templates ask only for
`--inspect` output and a file listing.

## License

Apache-2.0 — use it, change it, sell it, ship it inside something closed.

The names are the exception, and only because a name is the one thing that cannot be shared without
making the open half untrustworthy. If you fork and distribute, give it your own name; everything
else, including saying truthfully what you built on, needs no permission.
[TRADEMARK.md](TRADEMARK.md) is the whole of it.
