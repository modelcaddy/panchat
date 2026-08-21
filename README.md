# panchat

Read AI chat exports from any vendor into one representation — and say out loud what each export
left behind.

> **Status: v0.1, pre-1.0.** The shape may still move. The crate name is provisional pending a
> naming clearance check.

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
| ChatGPT | `conversations.json` | Conversations, branch graph, attachments (referenced), per-message model |
| Claude | `conversations.json` + `projects.json` + `memories.json` | Conversations, projects, memories, attachments (referenced) |

## Known lossiness, by vendor

The point of this table is that no vendor publishes it.

| | ChatGPT | Claude |
|---|---|---|
| Branch / regeneration history | present, preserved | not in export |
| Per-message model identity | present | **absent from the format** |
| Attachment bytes | not included, referenced only | not included, referenced only |
| Tool / code-interpreter payloads | partially typed, rest preserved verbatim | typed (`tool_use` / `tool_result`) |
| Timestamps | float unix seconds | ISO 8601 |
| Project membership | via template/gizmo id, name not included | full, with name |

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

## Design rules

1. A tiny required core — only what every vendor actually has.
2. Extension points reserved from v0.1, because they cannot be retrofitted.
3. Consumers must preserve fields they do not recognise.
4. Nothing app-specific in the core namespace.

## License

Apache-2.0.
