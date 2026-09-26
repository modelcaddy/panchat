# Claude Code session log

What Claude Code's local session history looks like on disk, and what the adapter does about it.
This is not a vendor export — nobody requests it and nothing emails it — it is the client's own
record of every session, kept in `~/.claude/projects/`. It is read as `method: capture`.

**Observed.** Unlike the Gemini log, this one was written from real data: the structure (key names,
value types, counts — never content) of 171 sessions and 139 subagent transcripts on the machine
the adapter was written on, in September 2026. The numbers below are from that survey.

| Shape | Emitted as | First seen | Mark |
|---|---|---|---|
| v1 | `local_history_v1` | 2026-09 | JSONL lines of `user`/`assistant` nodes carrying `uuid`, `parentUuid`, `sessionId`, `message` |

---

## v1 · 2026-09 — JSONL, one file per session

**Layout**

```text
~/.claude/projects/
  <cwd with / replaced by ->/
    <session-uuid>.jsonl                       one session
    <session-uuid>/subagents/agent-*.jsonl     transcripts of subagents that session ran
    <session-uuid>/subagents/…/journal.jsonl   workflow journals — not sessions
    <session-uuid>/tool-results/*.txt          tool output too large for the transcript
    memory/*.md                                memory files for that project
```

Point the tool at the whole store, at one project directory, or at one `.jsonl`. Detection reads the
first lines of each `.jsonl` for a `user` or `assistant` node with a `sessionId`, a `uuid` and a
`message`; the filename is never consulted, and a workflow journal is not mistaken for a session.

**What a line is**

| `type` | Count in sample | In the graph? | Handling |
|---|---|---|---|
| `assistant` | 99,633 | yes | a message; `message.model` present on every one |
| `user` | 62,216 | yes | a message; `isMeta` lines are hidden |
| `attachment` | 44,407 | **yes** | a hidden message carrying the vendor payload as `unknown` |
| `system` | 3,135 | **yes** | a hidden `system` message carrying the payload as `unknown` |
| `last-prompt` | 16,242 | no | its `leafUuid` is the active branch |
| `custom-title`, `ai-title` | 13,329 / 5,549 | no | the title — the user's own over the generated one |
| everything else (`pr-link`, `mode`, `queue-operation`, `bridge-session`, …) | | no | kept in `Conversation.raw` |

**Nodes that are not turns sit in the parent chain.** Attachments and system events carry a `uuid`
and are the parent of whatever comes next. An importer that keeps only user and assistant lines
leaves messages pointing at parents that are not there, so they are kept, hidden, with their payload
intact.

**One API message is several lines.** A single assistant response is written as one line per
content block — thinking, then a tool call, then text — sharing a `message.id` and chained by
`parentUuid`. Each line is kept as its own node. Merging them would be a reformat, and the graph is
defined on the lines, not on the API message.

**Blocks inside `message.content`**

| Block | Count | Handling |
|---|---|---|
| `tool_result` | 58,523 | `tool_result`, named after the tool its `tool_use_id` answers |
| `tool_use` | 58,511 | `tool_use` |
| `thinking` | 27,141 | `unknown`, verbatim — reasoning is not the answer |
| `text` | 14,772 | text |
| `image` | 787 | `unknown`, verbatim — the bytes are inline base64, and the representation has no field for inline bytes yet (SPEC open question 2) |
| `fallback` | 8 | `unknown`, verbatim |

A `content` that is a plain string (2,901 in the sample) is one text part.

## The graph

**It is a real graph.** 10,579 nodes in the sample have more than one child — every rewind, every
edited prompt, every retry is a sibling branch. That makes this the only source here, alongside
ChatGPT's export, where branches exist to lose, and so the only capture that **must not** warn
`branches_unavailable`: it could see them, and they are in the document. SPEC.md was amended to say
so, because as first written it required the warning from every capture.

**Compaction starts a new root.** 21 of 171 sessions have more than one root. After a context
compaction the next node has `parentUuid: null` and a `logicalParentUuid` naming the node it follows.
That link is the vendor's own, and following it gives every observed session exactly one root —
which is what lets `active_path` run from the first message to the last.

**No dangling parents were observed.** A parent that is not in the session would make that node a
root and warn `branch_pointer_broken`; the code is there, the sample never needed it.

**The active branch.** Every session had a `last-prompt` line and every `leafUuid` named a node that
existed — usually an assistant message (140 of 171). The active path is the walk up from it. A
subagent transcript has no `last-prompt`, but also never branches: with exactly one leaf, the path is
that leaf's, and no reconstruction is claimed. Only several leaves and no pointer produces a guess,
and the guess is reported.

## Rewrites

**A node can be written more than once.** 12,867 uuids in the sample appear on more than one line.
4,011 of the repeats are identical; almost all the rest differ only in session metadata — `slug`,
`promptId`, `gitBranch`, `cwd`, `version`, `toolUseResult` — as a resumed session rewrites earlier
nodes. Ids are unique within a document, so the last write wins. In 26 cases the rewrite changed
the `message` or the `parentUuid`; those are counted in an `unhandled_export_section` warning at
`lossy`, since the earlier write is not represented.

## What is not represented

| | Handling |
|---|---|
| `usage`, `stop_reason`, `requestId`, `toolUseResult`, per-line `cwd`/`gitBranch`/`version` | one folded `unhandled_export_section` (`lossy`) naming them — token counts are deliberately absent from the format, and the rest is envelope |
| `tool-results/*.txt` | counted, `unhandled_export_section` (`lossy`) — real output, referenced from the transcript, not read |
| non-session JSONL (workflow journals) | counted, `unhandled_export_section` (`info`) |
| the non-node lines of a session | kept whole in `Conversation.raw` |

Per-message lines are **not** kept in `raw`. The sample store is 1.9 GB with a 152 MB largest session,
and holding every line twice is the difference between reading it and not.

## Mapped rather than dropped

| Source | Becomes |
|---|---|
| the session's first `cwd` | `Conversation.project.id` — the store itself is keyed on it |
| `custom-title`, else `ai-title` | `Conversation.title`, with `x-panchat.claude_code_title_source` = `custom` or `ai`, because a consumer may care that a model wrote the title |
| `sessionId`, `isSidechain`, first `gitBranch` | `x-panchat.claude_code_session_id`, `…_sidechain`, `…_git_branch` |
| `memory/*.md` | an `Artifact` of kind `memory` |

Conversation ids are the file's path under the store without `.jsonl` — `project/session` or
`project/session/subagents/agent-x` — because a subagent transcript carries its parent's session id,
and the session id alone would collide.
