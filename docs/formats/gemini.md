# Gemini export log

What Google Takeout's Gemini Apps activity has looked like, and what the adapter does about it.
As with the other logs here, this is a record of observation rather than of documentation — Google
publishes no schema for it.

> **Read this first.** Unlike the ChatGPT and Claude logs, **no export was read to write this one.**
> The shape below is reconstructed from the source code of roughly twenty independent parsers that
> do read real exports, listed at the end. They agree closely on the keys and disagree about almost
> everything else, and the disagreements are recorded here rather than resolved by guessing.
>
> If you have a Gemini Takeout export, [issue #6](https://github.com/modelcaddy/panchat/issues/6)
> asks for `--inspect` output and the key names of one record with the values replaced. That would
> turn most of this page from inference into observation. Please do not attach the export.

| Shape | Emitted as | First seen | Mark |
|---|---|---|---|
| v1 | `takeout_myactivity_v1` | reconstructed, 2026-09 | an array of activity records naming Gemini in `header` or `products` |

---

## v1 — an activity log, not a chat export

**This is the thing to understand about Gemini and the reason its row in the lossiness table looks
the way it does.** Google does not export conversations. It exports *My Activity* — the same log
that records a search performed or a video watched — filtered to one product. A conversation is not
an object in this format. It is a pattern across rows, and Google does not always record enough to
reconstruct it.

**Where it comes from.** Takeout → My Activity → Gemini Apps, format JSON. The top-level "Gemini"
product in Takeout is something else: it exports Gems configuration, not conversations.

**Layout**

```text
Takeout/My Activity/Gemini Apps/MyActivity.json
```

The path is localized. A Greek export writes
`Takeout/Η δραστηριότητά μου/Εφαρμογές Gemini/Ηδραστηριότητάμου.json`, and every other Google
product in the same download writes its own `MyActivity.json` with the same record shape. **So the
filename identifies nothing**, which is why detection here tests the records instead: an array of
activity rows, at least one of which names Gemini or Bard in `header`, `products`, or `titleUrl`.
Claiming another product's file would turn somebody's search history into a chat transcript.

**A large account is split across several downloads.** Takeout numbers them —
`takeout-<timestamp>-001.zip`, `-002.zip` — and what people do with several downloads is put them
in one folder. A folder of archives is read as one export, on the same rule the archive reader uses
one level down: only when nothing in the folder yielded a JSON array is the payload assumed to be
inside the archives. A folder holding an unpacked export and a zip is left alone.

**A record**

| Key | What it holds | Handling |
|---|---|---|
| `header` | `"Gemini Apps"` — localized, e.g. `"Gemini アプリ"` | identifies the product; matched as a substring |
| `title` | the user's prompt, behind a localized prefix | the user turn, prefix stripped |
| `time` | ISO 8601 UTC with `Z`, sometimes with milliseconds | `created_at` on both turns |
| `safeHtmlItem[].html` | the model's answer, as **HTML** | the assistant turn, **verbatim** |
| `titleUrl` | a `gemini.google.com/app/c/<id>` link, often absent or `null` | the conversation id when present |
| `products` | `["Gemini Apps"]`, localized | product identification |
| `activityControls` | which activity setting recorded this | not read |
| `attachedFiles[].name`, `attachmentInfo[]`, `imageFile` | files the user attached | attachments, bytes never included |
| `subtitles[]` | **meaning not stable** — see below | not read |

**The prompt prefix is localized and undocumented.** Parsers in the wild strip `"Prompted "`,
`"Asked "`, `"Said "`, `"Submitted query "`, Japanese `"送信したメッセージ: "`, Greek
`"Υποβλήθηκε το ερώτημα "`, and Spanish `"Has dicho: "` / `"Hiciste la petición "`. That list is
this adapter's list too, and it is certainly incomplete. A record whose prefix is not on it is
**not** discarded and its title is **not** guessed at — the title is taken whole. Worst case, a few
words of the user's own language sit in front of their own question; nothing is lost and nothing is
invented. If your locale is missing, that is a one-line pull request.

**`subtitles` is deliberately not read.** Its meaning is genuinely contested: some parsers read
`subtitles[].value` as the *prompt*, others read `subtitles[].name` as the *response*, others as
attachment notices, others as Canvas content. No two agree, and no fixture was found showing it
populated alongside `safeHtmlItem`. Reading it on a guess would put the wrong text in the wrong
turn, which is worse than not reading it. It survives in `Conversation.raw`.

## What this format cannot express, and what the adapter does about it

| | In the export | What happens |
|---|---|---|
| **Conversation grouping** | only `titleUrl`, and often not that | grouped on `titleUrl` where Google supplies one; every other record stands alone |
| **Conversation id** | none | synthesized from `time` and `title`, `synthesized_id` (`info`) |
| **Title** | none | left absent — never synthesized from the first prompt |
| **Per-message model** | none | `no_model_identity` (`info`) |
| **Branches, regenerations** | none | nothing to report: the format has no concept of one |
| **Attachment bytes** | referenced only | `attachment_not_included` (`lossy`) |
| **Non-chat activity** | canvas, image generation, feedback, draft selection | counted and named, `unhandled_export_section` (`lossy`) |

**Grouping is where a tool is most tempted to invent.** Several parsers in the wild stitch rows
into conversations on a time gap, usually thirty minutes. That produces a conversation Google never
recorded, and once it is written down nobody downstream can tell it from a real one. This adapter
does not do it. Where Google gives a pointer it is used; where it does not, one record is one
conversation, and the lossiness table says so.

**Ids are derived from the record, never from its position.** Takeout prepends new activity, so a
fresh export shifts every index. An id built from position would re-import an entire history as
new; one built from `time` and `title` de-duplicates instead. This is the same reasoning every
careful parser in the field arrived at independently.

**The answer stays HTML.** Google stores the response as HTML, and every parser found converts it —
to text, to Markdown, with hand-rolled regexes. The specification this crate implements forbids a
producer from reformatting what a vendor stored, so the HTML is passed through and a consumer
decides. That is a real ergonomic cost and it is the honest choice: a Markdown conversion is an
opinion, and opinions do not belong in a parse.

## Reported, and not implemented

Two other record layouts are described by parsers in the field. Both are backed only by
hand-written synthetic fixtures — no evidence of a real export producing either was found — so
neither is implemented, because writing a parser against invented evidence is the failure this
crate exists to avoid.

- **`details[]` with `name: "Request"` / `"Response"` and a `value`**, under a title of
  `"Used Gemini Apps"`.
- **`userInteractions[].userInteraction.request` / `.response`**, whose values are *serialized JSON
  strings* that must be parsed again to reach the text.

A record in either shape currently falls out as non-conversational and is counted in the
`unhandled_export_section` warning, so it is reported rather than silently dropped. If you hold an
export containing one, that is the evidence needed to implement it.

## Still unconfirmed

Written down because an implementer will hit them, and because the next person to read a real
export can close them:

1. Whether `title` truncates a long prompt. No source documents a cap from Google's side.
2. Whether `subtitles[].value` and `safeHtmlItem` ever appear on the same record.
3. Whether `time` is unique in general. One measured export of 1,452 records had no duplicates, and
   this adapter's ids depend on it; a collision would fold two records together.
4. The structure of `attachedFiles` and `imageFile` beyond a name, and whether the attached bytes
   are anywhere in the Takeout download.
5. Which locales exist beyond the four whose prefixes are handled.
6. Whether the `{"conversations": [...]}` per-conversation shape some tools describe is real. Both
   sources for it are unimplemented proposals.

## Sources

The shape above was reconstructed by reading the parsing code of, among others:
`seanrobertwright/420AI` (which documents a structure-only inspection of a real 1,452-record
export), `Syun-tnb/llm-logparser`, `osaurus-ai/osaurus`, `Jyot-Pandya/OrgChat`,
`QSOLKCB/AI-CONTEXT`, `aarzamen/constellation-v3`, `Coryrichter94/gemini-to-obsidian`,
`marswangyang/personal-ai-memory`, `NateBJones-Projects/OB1` (which reports 1,161 of 1,640 records
carrying a response), `minipoisson/Gemini_Json2md4NotebookLM`, `l33tdawg/sage`, `cgpp5/Re.mind`,
`silver-gr/AI-Conversation-Toolkit`, `AminaEmenena/data-extractor`, and
`purarue/google_takeout_parser`.

Worth recording, because it is the failure mode this crate is about: `google_takeout_parser`, the
most widely used generic Takeout library, routes Gemini activity through its generic parser, which
has no `safeHtmlItem` branch and whose subtitle model has no `value` field. It reads these files
without complaint and silently drops every answer.
