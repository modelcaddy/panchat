# AI Chat Interchange — format specification

**Version 0.1 · pre-1.0 · 2026-08-20**

This document specifies the JSON shape produced and consumed by `panchat`. It is written for a
third party implementing a producer (a new vendor adapter, or a tool emitting its own
conversations) or a consumer (a viewer, an analysis pipeline, another memory system).

## Status and stability

This is a **0.x** version. The shape may change in a way that breaks consumers, and `0.2` is not
promised to read `0.1`. It is published now so implementations can begin and so the shape gets
exercised before it is frozen — not because it is settled.

**1.0** will carry a compatibility promise: additive changes only within a major version. Until
then, pin the version you tested against and read `format_version` at runtime.

The normative artifacts are this document and `schema/chat-v0.1.json`. Where they disagree, the
JSON Schema wins for structure and this document wins for meaning.

## Conformance language

**MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used as in RFC 2119.

## Serialization

JSON, UTF-8, no BOM.

| Form | File extension | Contents |
|---|---|---|
| Document | `.chat.json` | One `Document` object |
| Conversation stream | `.chat.jsonl` | One `Conversation` object per line, newline-delimited |
| Bundle | `.chat` | Zip containing `document.chat.json` plus attachment bytes under `attachments/` |

The bundle form exists because a conversation with attachments cannot be a single JSON file. Only
the bundle form may populate `ContentPart.Attachment.path`.

> **Implementation status:** the reference implementation reads and writes the `.chat.json` and
> `.chat.jsonl` forms. The `.chat` bundle is specified but **not yet implemented** — no producer
> currently emits `path`, and every attachment is reported `attachment_not_included`.

There is no registered media type yet; `application/json` is correct in the meantime.

---

## The extension model

Three mechanisms, and every producer and consumer depends on all three. **These are the rules that
make the format survivable across versions. An implementation that ignores them is not
conformant.**

### 1. Unknown keys MUST be preserved

A consumer that reads a document, modifies it, and writes it back **MUST** carry through every key
it did not recognise, at every level. This is the mechanism by which a producer's richer data
survives a trip through a simpler tool.

Do not build a consumer that deserializes into a fixed struct and re-serializes. Keep the original
object, or keep an overflow map.

### 2. Third-party keys MUST be namespaced

Any key not defined by this specification **MUST** be prefixed `x-`, followed by a stable
identifier for the producer: `x-modelcaddy`, `x-acme`. Unprefixed keys are reserved for future
versions of this specification.

Namespaced keys may appear on `Document`, `Conversation`, and `Message`. Their values are
unconstrained.

```json
{
  "id": "conv-1",
  "messages": [],
  "x-modelcaddy": { "space": "ascii", "ref_num": 12 }
}
```

### 3. Unrecognised content parts MUST be preserved verbatim

A producer that meets a message part it cannot interpret **MUST** emit
`{"type": "unknown", "kind": "<vendor label>", "raw": <the original payload>}` rather than dropping
it or substituting a placeholder string. A consumer **MUST** treat `unknown` as opaque and carry it
through.

This is what lets an out-of-date parser degrade instead of destroying data.

---

## Object: Document

The top-level object. One document holds one export.

| Property | Type | Required | Meaning |
|---|---|---|---|
| `schema` | string (URI) | ✅ | URI of the JSON Schema this document claims to conform to. |
| `format_version` | string | ✅ | `"0.1"`. Consumers **MUST** check this before interpreting anything else. |
| `source` | `Source` | ✅ | Where the document came from. |
| `conversations` | `Conversation[]` | — | Omitted when empty. |
| `artifacts` | `Artifact[]` | — | Non-conversation items shipped in the same export. Omitted when empty. |
| `warnings` | `Warning[]` | — | What the producer could not faithfully represent. Omitted when empty. |
| `x-*` | any | — | Namespaced extensions. |

`warnings` lives on the document rather than being returned out of band so that a serialized file
still carries its own honesty record. A file that has been copied between three tools still says
what its original export was missing.

## Object: Source

| Property | Type | Required | Meaning |
|---|---|---|---|
| `platform` | string | ✅ | `chatgpt`, `claude`, … |
| `method` | string | — | `export` or `capture`. See *Acquisition methods*. |
| `variant` | string | — | Which shape was recognised, e.g. `official_export_v1`. |
| `exported_at` | string (RFC 3339) | — | When the vendor generated the export, if it says. |

`platform` is a **free string, not an enumeration**. A producer for a vendor this specification has
never heard of must be expressible without a specification change. Consumers **MUST NOT** reject a
`platform` they do not recognise.

## Acquisition methods

Data reaches this format two ways, and **they are lossy in opposite
directions**. A consumer that ignores `method` will misread its own data.

| | `export` | `capture` |
|---|---|---|
| What it is | The vendor's own data export | Read from a live page, or a client's local history |
| Platform coverage | Only vendors that offer one | Anything with a web UI |
| Latency | Vendor-controlled; ChatGPT's arrives by email, often hours later | Immediate |
| Selectivity | All or nothing | Per conversation, per project |
| **Alternative branches** | **Present** — the export ships the whole graph | **Invisible** — a page renders one branch |
| Vendor ids | Stable | Often absent or derived |
| Attachment bytes | Sometimes included | Fetchable while the session is live |
| Completeness | Everything the vendor holds | Only what was rendered and read |

The consequence a consumer must handle: **an empty `active_path` and a
branch-free `messages` array mean different things under each method.** Under
`export` it means the user never regenerated an answer. Under `capture` it means
the alternatives could not be seen. Producers using a capture method therefore
**MUST** emit a `branches_unavailable` warning, so the difference is on the
record rather than inferred.

`method` is a free string like `platform`. `export` and `capture` are defined;
consumers **MUST NOT** reject others. Absent `method` **SHOULD** be treated as
the weaker guarantee — assume branches may be missing.

## Object: Conversation

| Property | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | ✅ | Stable within the source platform. See *Identity* below. |
| `title` | string | — | Absent when the vendor has none. Never synthesized from content. |
| `created_at` | string (RFC 3339) | — | |
| `updated_at` | string (RFC 3339) | — | |
| `project` | `ProjectRef` | — | Vendor-side project, folder, or space membership. |
| `url` | string (URI) | — | Canonical vendor URL, when derivable. |
| `messages` | `Message[]` | ✅ | **Every** message, including off-path branches. May be empty. |
| `active_path` | string[] | — | Message ids, root-first, forming the branch the vendor marked current. |
| `raw` | any | — | The untouched vendor payload for this conversation. |
| `x-*` | any | — | Namespaced extensions. |

### The message graph — read this before implementing a consumer

`messages` is a **flat array carrying a graph**, not an ordered transcript. Each message has an
optional `parent`. Messages sharing a parent are siblings: alternative continuations from the same
point, produced when a user regenerates an answer or edits an earlier prompt.

`active_path` names the single branch the vendor considered current — what the user saw last.

A consumer that wants a linear transcript **MUST** walk `active_path`, not iterate `messages`.
Iterating `messages` yields sibling branches interleaved, which reads as duplicated turns.

```
n1 (user)
├── n2 (assistant)   ← regenerated away, still present
└── n3 (assistant)   ← on active_path
    └── n4 (user)
```

Rules:

- `active_path` **MUST** contain only ids present in `messages`.
- `active_path` **MUST** be ordered root-first and form a parent-child chain.
- An **empty** `active_path` means the source gave no branch signal. Consumers **SHOULD** then
  treat `messages` order as the transcript order, and producers **SHOULD** emit a
  `branch_pointer_broken` warning.
- A source with no branching (most vendors) **SHOULD** still populate `active_path` with every
  message id, so consumers need only one code path.
- `parent` **MUST** either be absent or name an id present in `messages`. Producers **MUST NOT**
  emit a pointer to a node they did not include. Consumers **SHOULD** tolerate a dangling pointer
  by treating that message as a root.
- The ordering of `messages` itself is **not** normative. Producers SHOULD order by timestamp then
  id for determinism; consumers MUST NOT depend on it.

### Identity

`id` **MUST** be stable across repeated exports of the same conversation from the same platform —
that is what makes de-duplication on re-import possible. Producers **SHOULD** use the vendor's own
id. When no vendor id exists, a producer **MAY** synthesize one and **MUST** emit a
`synthesized_id` warning.

Ids are unique within a document. They are **not** globally unique: two platforms may collide.
Consumers needing a global key **SHOULD** use `(source.platform, conversation.id)`.

## Object: ProjectRef

| Property | Type | Required |
|---|---|---|
| `id` | string | ✅ |
| `name` | string | — |

Some vendors expose only an opaque project id with no name. `name` is absent in that case rather
than filled with the id.

## Object: Message

| Property | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | ✅ | Unique within the conversation. |
| `parent` | string | — | Parent message id. Absent marks a root. |
| `role` | string | ✅ | See *Role*. |
| `created_at` | string (RFC 3339) | — | |
| `model` | string | — | Which model produced this message. |
| `content` | `ContentPart[]` | — | Omitted when empty. |
| `hidden` | boolean | — | Default `false`. Omitted when false. |
| `x-*` | any | — | Namespaced extensions. |

`model` is **per message, not per conversation**. A single thread routinely spans several models,
and which model produced a given answer is often the reason someone kept the conversation. Most
export formats do not record it; producers for those formats emit a `no_model_identity` warning.

`hidden` marks a turn the vendor did not show the user — system framing, tool plumbing. These are
**recorded, not dropped**: a hidden system prompt is frequently the most valuable line in an export.
Renderers **SHOULD** omit hidden messages from a reading view by default; the choice belongs at
render time, not parse time.

## Role

A free string. Four values are defined:

| Value | Meaning |
|---|---|
| `user` | The human. |
| `assistant` | The model. |
| `system` | Instructions framing the conversation. |
| `tool` | Output of a tool or function call. |

Producers **MUST** normalize a vendor's own vocabulary onto these where the meaning matches
(`human` → `user`, `model`/`bot` → `assistant`, `function` → `tool`).

Producers **MAY** emit any other string for a role with no equivalent. Consumers **MUST NOT** reject
an unrecognised role, and **SHOULD** render it as the literal string.

## ContentPart

A message's content is an **ordered array of typed parts**, not a string. This is the only way to
represent a turn that interleaves text with an image, or text with a tool call, without inventing
in-band markers.

Every part has a `type`. Five are defined.

### `text`

| Property | Type | Required |
|---|---|---|
| `type` | `"text"` | ✅ |
| `text` | string | ✅ |

Content is whatever the vendor stored, usually Markdown. Producers **MUST NOT** reformat it.

Multiple consecutive `text` parts are permitted; consumers joining them **SHOULD** use a newline.

### `attachment`

| Property | Type | Required | Meaning |
|---|---|---|---|
| `type` | `"attachment"` | ✅ | |
| `id` | string | — | Vendor's file identifier. |
| `name` | string | — | Original filename. |
| `mime_type` | string | — | A real media type, or absent. |
| `path` | string | — | Path inside the bundle. Present **only** when the bytes shipped. |
| `size_bytes` | integer | — | |

**Absence of `path` means the bytes are not available.** Nearly every vendor export references
files it does not include; a producer in that situation **MUST** emit an
`attachment_not_included` warning so a consumer can tell the user rather than silently rendering a
broken image.

`mime_type` **MUST** be a real media type or absent. Producers **MUST NOT** put a vendor's internal
part label there — a consumer will act on it.

### `tool_use`

| Property | Type | Required |
|---|---|---|
| `type` | `"tool_use"` | ✅ |
| `name` | string | — |
| `input` | any | — |

### `tool_result`

| Property | Type | Required |
|---|---|---|
| `type` | `"tool_result"` | ✅ |
| `name` | string | — |
| `output` | any | — |

`input` and `output` are unconstrained JSON — vendors disagree on their shape, and normalizing
would lose more than it gains.

### `unknown`

| Property | Type | Required | Meaning |
|---|---|---|---|
| `type` | `"unknown"` | ✅ | |
| `kind` | string | — | The vendor's own label for the part. |
| `raw` | any | ✅ | The original payload, untouched. |

Emitted when a producer meets a part shape it does not model. See extension rule 3. A producer
emitting `unknown` **MUST** also emit an `unknown_content_part` warning.

## Object: Artifact

Non-conversation items a vendor ships in the same export — Claude projects and memories, custom
instructions.

| Property | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | ✅ | |
| `kind` | string | ✅ | `project`, `memory`, `instruction`, or vendor-specific. |
| `title` | string | — | |
| `text` | string | — | |
| `created_at` | string (RFC 3339) | — | |
| `raw` | any | — | Untouched vendor payload. |

Deliberately loose. Modelling every vendor's side-cars properly is out of scope for 0.1; dropping
them would lose the most useful part of some exports, so they are carried in a shallow shape with
`raw` intact.

## Object: Warning

The distinguishing feature of this format: **a document states what it could not represent.**

| Property | Type | Required | Meaning |
|---|---|---|---|
| `code` | string | ✅ | Stable identifier. See the table. |
| `severity` | string | ✅ | `info`, `lossy`, or `dropped`. |
| `conversation_id` | string | — | When the warning is item-specific. |
| `message_id` | string | — | |
| `count` | integer | — | Default `1`. Repeats of a non-item-specific code are folded. |
| `detail` | string | — | Free text. **Never the only carrier of meaning** — `code` is. |

### Severity

| Value | Meaning |
|---|---|
| `info` | Something is absent that never existed in the source. **Nothing was lost.** |
| `lossy` | Something in the source was not fully represented. |
| `dropped` | Something in the source was discarded. |

The `info` / `lossy` distinction matters and is frequently got wrong. "Claude exports contain no
per-message model" is `info` — the data never existed. "This attachment's bytes are not in the
export" is `lossy` — the file exists, you just do not have it.

### Codes

| Code | Typical severity | Meaning |
|---|---|---|
| `missing_timestamps` | `info` | The source records no timestamps for this item. |
| `synthesized_id` | `info` | No stable id in the source; one was generated. |
| `no_model_identity` | `info` | The format does not record which model produced each message. |
| `unknown_content_part` | `lossy` | A part was not recognised and was kept verbatim as `unknown`. |
| `attachment_not_included` | `lossy` | A file is referenced but its bytes are not in the export. |
| `branch_pointer_broken` | `lossy` | The active-branch pointer was missing or broken; order is reconstructed. |
| `branch_cycle` | `lossy` | A cycle in the message graph was cut. |
| `branches_unavailable` | `lossy` | The acquisition method cannot observe alternative branches. |
| `unhandled_export_section` | `info` | Part of the export is not handled by this producer version. |
| `item_skipped` | `dropped` | An item was too malformed to parse and was skipped. |

New codes are **additive**. Consumers **MUST** tolerate a code they do not recognise — display
`severity` and `detail`, do not fail.

Renaming or repurposing an existing code is a **breaking change**.

---

## Producer conformance

A conformant producer:

1. **MUST** emit `schema`, `format_version`, and `source`.
2. **MUST** preserve every message, including off-path branches, when the source has them —
   and **MUST** warn `branches_unavailable` when its method cannot see them at all.
3. **MUST** emit `unknown` parts rather than placeholder text for shapes it cannot interpret.
4. **MUST** emit a warning for every lossy or dropped condition it detects.
5. **MUST NOT** invent data — no synthesized titles, no guessed timestamps, no `mime_type` derived
   from a vendor's internal label.
6. **SHOULD** populate `raw` on each conversation. It is the round-trip insurance for everything
   this version failed to model.
7. **SHOULD** be pure with respect to its inputs — no filesystem, no clock — so it is
   fixture-testable. Silent rot when a vendor changes an export is the primary failure mode of
   software in this category, and fixtures are the only defence.
8. **SHOULD NOT** fail a whole export because one item is malformed. Skip the item, warn
   `item_skipped`, continue. One bad conversation must never cost the user the other 9,999.

## Consumer conformance

A conformant consumer:

1. **MUST** check `format_version` before interpreting the document.
2. **MUST** preserve unknown keys, unknown roles, and `unknown` parts on any read-modify-write.
3. **MUST** walk `active_path` for a linear transcript, not iterate `messages`.
4. **MUST NOT** reject an unrecognised `platform`, `role`, `kind`, or warning `code`.
5. **SHOULD** surface warnings to the user rather than discarding them. A consumer that hides
   `attachment_not_included` and renders a broken image is worse than one that says the file is
   missing.

## Deliberately absent

Not oversights:

- **Embeddings, summaries, extracted tasks, or any derived data.** This format carries what a
  vendor exported, not what a tool inferred from it. Derived data belongs under an `x-` key.
- **A user or account object.** Exports contain personal data; this format does not ask for more of
  it than the conversation requires.
- **Token counts and cost.** Vendor-specific, unstable, and reconstructible.
- **A canonical vendor enumeration.** See `Source.platform`.
- **A round-trip back to a vendor's own export format.** No vendor imports its own export.

## Worked example

A ChatGPT export where the user regenerated one answer and attached an image. `raw` is elided for
length; a real document carries it.

```json
{
  "schema": "https://modelcaddy.github.io/panchat/schema/chat-v0.1.json",
  "format_version": "0.1",
  "source": { "platform": "chatgpt", "variant": "official_export_v1" },
  "conversations": [
    {
      "id": "conv-1",
      "title": "Auth refactor",
      "created_at": "2025-06-15T15:06:40.500+00:00",
      "messages": [
        {
          "id": "n0",
          "role": "system",
          "created_at": "2025-06-15T15:06:40+00:00",
          "content": [{ "type": "text", "text": "You are helpful." }],
          "hidden": true
        },
        {
          "id": "n1",
          "parent": "n0",
          "role": "user",
          "content": [{ "type": "text", "text": "How do I rotate tokens?" }]
        },
        {
          "id": "n2",
          "parent": "n1",
          "role": "assistant",
          "model": "gpt-4o",
          "content": [{ "type": "text", "text": "First answer, later regenerated." }]
        },
        {
          "id": "n3",
          "parent": "n1",
          "role": "assistant",
          "model": "gpt-5.2",
          "content": [{ "type": "text", "text": "Second answer, the one kept." }]
        },
        {
          "id": "n4",
          "parent": "n3",
          "role": "user",
          "content": [
            { "type": "text", "text": "See this diagram:" },
            { "type": "attachment", "id": "file-service://abc", "size_bytes": 4096 }
          ]
        }
      ],
      "active_path": ["n0", "n1", "n3", "n4"]
    }
  ],
  "warnings": [
    {
      "code": "attachment_not_included",
      "severity": "lossy",
      "conversation_id": "conv-1",
      "message_id": "n4",
      "count": 1
    }
  ]
}
```

`n2` is in `messages` and absent from `active_path`: the answer the user regenerated away. A
transcript view shows four turns; nothing was destroyed to produce it.

## Open questions for 1.0

Named here because an implementer will hit them:

1. **Should `active_path` be required?** Making it required forces every producer through one code
   path and removes an empty-case branch from every consumer. It also forces flat-export producers
   to write a list they do not need.
2. **Attachment bytes** are addressed only by the bundle form. Whether to allow inline base64 for
   the single-file form is unresolved.
3. **Threading across conversations** — a "continue in a new chat" link — is not modelled. Vendors
   are beginning to record it.
4. **Artifact modelling** is deliberately shallow and will need revisiting once more than two
   vendors are supported.
