# Claude export log

What Anthropic's data export has actually looked like, each time one was read, and what the adapter
does about it. As with the ChatGPT log, this is a record of observation, not of documentation — the
export shape is not published, and the fixture tests are what catch it moving.

Entries are newest first.

Each shape gets a number, and a document produced from it says which one it was:
`source.variant_version` in the emitted JSON, `variant` for the same thing in words. The numbering is
ours, not Anthropic's, and shares nothing with ChatGPT's — `variant_version` only means something
next to `platform`.

| Shape | Emitted as | First seen | Mark |
|---|---|---|---|
| v2 | `official_export_v2` | 2026-08-22 | `projects/` or `design_chats/` directories, or a `memories.json` holding `conversations_memory` |
| v1 | `official_export_v1` | before 2026 | flat `projects.json` and memory rows with ids |

An account with no projects and no design chats still gets the newer `memories.json`, which is why
the directories are not the only test.

---

## v2 · 2026-08-22 — side-cars in directories, design chats, new memories shape

**Observed:** a 1.6 MB export delivered as `data-<uuid>-<epoch>-<hash>-batch-0000.zip`, and the
directory unpacked from it.

**Layout**

```text
conversations.json          the familiar array; rows gained a `summary` field
projects/<uuid>.json        one file per project, each with docs[]
design_chats/<uuid>.json    Claude Design canvas chats — a second chat format
memories.json               one object, no longer a list of memory rows
users.json                  account identity
login_history.json          IP, user agent, and location per login
```

The directory name ends `batch-0000`, so Claude batches large exports the way ChatGPT shards them.
Only one batch was present here; reading several batches as one export is **not** implemented —
point the tool at each batch directory.

**What changed from the older shape**

| | Older | This export |
|---|---|---|
| Projects | `projects.json`, one array | `projects/<uuid>.json`, one file each, `docs[]` inline |
| Memories | list of rows, each with a `uuid` | one object: `conversations_memory` prose plus `memory_files[{path, content}]` |
| Design chats | absent | `design_chats/`, 5 chats, 127 messages |
| Other side-cars | `users.json` | `users.json`, `login_history.json` |
| Conversation rows | `uuid`, `name`, `chat_messages` | the same, plus a generated `summary` |

**Blocks inside `chat_messages[].content`**

| Type | Count | Handling |
|---|---|---|
| `text` | 16 | text |
| `thinking` | 15 | preserved verbatim as `unknown`, same policy as ChatGPT's `thoughts` |
| `tool_use` | 13 | `tool_use` |
| `tool_result` | 13 | `tool_result` |

**Design chats** speak a different dialect: `messages` rather than `chat_messages`, `content` as an
object rather than an array, and typed blocks under `contentBlocks`.

| Block | Count | Handling |
|---|---|---|
| `tool_call` | 370 | `tool_use`, plus `tool_result` when the call carries output |
| `thinking` | 146 | `unknown`; empty placeholders are dropped, since nothing was lost |
| `text` | 80 | text |
| `error` | 8 | `unknown` — upstream failures, kept verbatim |
| `user_interjection` | 5 | `unknown` |

Message `kind`s seen: `chat`, `question-record`, `question-receipt`, `questions-response`,
`direct-edit`. Their prose lives in `content.content`, so they read as ordinary turns.

Design attachments (80) carry their text inline rather than referencing bytes: the text becomes a
text part, and only the 31 with no inline content are reported `attachment_not_included`.

Design chats are marked `x-panchat.claude_export_section = "design_chats"` on the conversation.
They are all titled "Chat", carry no model identity, and behave differently enough from a normal
conversation that a consumer needs to be able to tell them apart.

**Result of reading it**

- 6 conversations (1 chat, 5 design chats), 10 artifacts (2 projects, 1 project document, 7
  memories), 62 warnings.
- Emitted as `variant: "official_export_v2"`, `variant_version: 2`.

**What it cost before the adapter changed:** 1 conversation and **zero** artifacts. The design chats
were invisible, and the memories were dropped without a warning because parsing required a row
`uuid` the new shape does not have.

**Skipped deliberately:** `users.json`, `login_history.json` — account and security metadata, not
conversation data. Each is reported `unhandled_export_section` (`info`).

**Still not modelled:** the conversation-level `summary` (kept in `Conversation.raw`), and the
design chats' `questionRecord` / `turnChanges` structures (kept in the message's raw block).

---

## v1 · before 2026 — flat side-cars

**Layout:** `conversations.json` + `projects.json` + `memories.json` (+ `users.json`).

Conversations are a flat `chat_messages` array — no branch graph, so no branch data to lose — and no
per-message model identity, reported once per document as `no_model_identity` (`info`: the data
never existed, so nothing was lost).

Read as variant `official_export_v1`, `variant_version: 1`. Still supported, including an export
that mixes both layouts — a mixed export reads as v2, since that is the newest shape present.
