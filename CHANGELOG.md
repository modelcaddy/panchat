# Changelog

The format of this file follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
crate follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html) — with the caveat that
`0.x` makes no compatibility promise, in the crate's API or in the interchange format it emits.

Vendors change their exports without announcing it, so a change here is often a reaction to a shape
that appeared in the wild. What each vendor's export looked like on the day it was read, and what it
cost us, is recorded per source:

- [ChatGPT export log](docs/formats/chatgpt.md)
- [Claude export log](docs/formats/claude.md)

## Unreleased

### Added

- **An archive of archives is read as the export inside it.** A large enough account does not
  receive one zip; it receives a zip of part archives, and read flat that is a download containing
  no export at all. One level of nesting is now followed, and exactly one, with the parts merged as
  though each had been unpacked into the same folder — so a split export and the folder it unpacks
  to still produce the same document, which is the property the whole archive path exists for.
  Whether to look inside is decided by shape rather than by a filename: an archive that already
  yielded a JSON array holds its payload, and is left alone. That is what keeps a zip the *user*
  uploaded — 2026 exports ship those bytes — from being opened and sprayed across their export.
  The delivery changed, not the shape, so an export that arrives this way is still
  `official_export_v2`. See [docs/formats/chatgpt.md](docs/formats/chatgpt.md), which also records
  that this layout has been reported rather than observed here.
- **Gemini, via Google Takeout.** The third platform, and the one that most tests what this
  representation is for: Google does not export conversations, it exports *My Activity* — the same
  log that records a search — filtered to one product. So there is no conversation object, no id,
  no title, no model, and no thread. Every one of those absences is reported rather than papered
  over, and the two temptations are refused: rows are **not** stitched into conversations on a time
  gap, because that invents a conversation Google never recorded, and the answer's HTML is **not**
  converted to Markdown, because a producer must not reformat what a vendor stored. Grouping uses
  `titleUrl` where Google supplies one; ids are derived from the record's own content so a
  re-export de-duplicates rather than doubling. Detection tests the records, never the filename:
  every Google product writes its own `MyActivity.json` into the same download, and the path is
  localized. See [docs/formats/gemini.md](docs/formats/gemini.md) — including the warning that this
  shape was reconstructed from twenty other parsers rather than read from an export, and the list of
  what is still unconfirmed.
- **ROADMAP.md**, and the response-time note contributors were entitled to ask for. What is planned
  and in what order now has an answer in the repository rather than in the maintainer's head.
- **[TRADEMARK.md](TRADEMARK.md)** — what the license does not cover, which is the names. Forking is
  encouraged and saying truthfully what you built on needs no permission; distributing a modified
  version under this name is the only thing asked against. It deliberately is **not** a `NOTICE`
  file: Apache-2.0 section 6 makes reproducing `NOTICE` content an express exception to the
  trademark non-grant, so a name protected there would be a name licensed away, and section 4(d)
  would propagate it into every derivative work forever. Section 4(c), which retains trademark
  notices found in the source form, is the mechanism that actually fits.

### Changed

- **The decompression budget is shared across a whole read** rather than granted afresh to each
  archive, since several archives handed over together are one export and the budget is a statement
  about memory. An entry that would take a read past it now names itself in the error, because with
  parts inside parts "the archive" is ambiguous.
- **`homepage` and `documentation` on the crate.** Both were empty on crates.io, which reads as a
  project with nowhere to go.

## 0.3.0 — 2026-08-27

### Added

- **Zip archives are read in place**, under the new `zip` feature — on by default for the `cli`
  feature, off for the library so the dependency stays opt-in. The archive the vendor hands you and
  the directory it unpacks to produce identical documents: entry paths are matched to the unpacked
  layout by stripping the vendor's single wrapper folder, and the same rule decides which files are
  worth loading, so attachment blobs inside an archive are referenced rather than decompressed.
  Archives are recognised by content, not by extension. `normalize` expands one handed to it
  directly, so a caller who loaded the bytes over HTTP or out of a database gets the same behaviour
  as one who passed a path. Without the feature, nothing changes: an archive is still named and the
  error still says to unpack it.
- **A decompression budget.** An archive is the one input that can lie about its size, so no more
  than 2 GiB of readable files is loaded from one, and reads are bounded by what is left of that
  budget rather than by the size the archive claims.
- **Contributor documentation.** [CONTRIBUTING.md](CONTRIBUTING.md) walks through writing an
  adapter and states what review checks; issue templates cover a vendor changing its export and
  adding a platform. Both templates ask for `--inspect` output and a file listing rather than an
  export, because an export contains every word its owner ever typed into that product.

### Changed

- **SPEC: what an attachment path means when the export is still zipped.** A producer reading an
  archive SHOULD emit the path the unpacked export would have had, minus the vendor's wrapper
  folder, so that the same export yields the same document either way.
- **CI runs the default-feature test suite as well as `--all-features`.** Features gate behaviour
  here rather than only compilation, so a mistake in a `#[cfg]` is invisible to `--all-features`
  alone — as one was: an existing test asserted advice that the `zip` feature makes wrong.

## 0.2.0 — 2026-08-27

The first release published to crates.io. `0.1.0` never left this repository.

### Added

- **ChatGPT: sharded exports.** Large accounts now export `conversations-000.json` …
  `conversations-NNN.json` with an `export_manifest.json` naming the shards. Every shard is read, in
  manifest order. Detected as variant `official_export_v1_sharded`; a shard the manifest lists but
  the download does not contain is reported `unhandled_export_section` (`dropped`).
- **ChatGPT: attachments resolved to the bytes the export ships.** `ContentPart::Attachment.path`
  now points at the file in the export, and `name` carries the original filename from
  `conversation_asset_file_names.json`. Both the current `<asset id>.dat` layout and the older
  layout that keeps the name on the file (`abc-diagram.png`, `dalle-generations/…`) are resolved.
  `attachment_not_included` is emitted only when the bytes really are absent.
- **ChatGPT: uploads listed in `metadata.attachments`.** Files the user attached appear only in
  message metadata, never in the content parts. They are now emitted as attachments, with the
  vendor's real `mime_type` and size, deduplicated against asset pointers in the same message.
- **ChatGPT: voice and video parts.** `audio_transcription` parts become text — the transcript is
  the turn — and the pointers nested inside `real_time_user_audio_video_asset_pointer` become
  attachments instead of one opaque blob.
- **Claude: directory side-cars.** `projects/<uuid>.json` is read alongside the older
  `projects.json`, and each project's `docs[]` becomes a `project_doc` artifact.
- **Claude: the newer `memories.json`.** One object holding `conversations_memory` and
  `memory_files[]`, rather than a list of memory rows with ids. The old shape is still read; an
  unrecognised one is now reported instead of silently yielding nothing.
- **Claude: `design_chats/`.** Claude Design's canvas chats are read as conversations, with
  `tool_call` blocks typed as `tool_use`/`tool_result` and attachment text kept inline. They are
  marked `x-panchat.claude_export_section = "design_chats"`, which a consumer cannot otherwise infer.
- **Claude: exports with both layouts** — flat side-cars and directories — are read whole.
- **Zip archives are named, not rejected as junk.** `normalize` recognises the archive magic and
  says to unpack it, since that is the commonest way an import fails.
- **Unread export sections are reported.** `user.json`, `message_feedback.json`,
  `model_comparisons.json`, `shared_conversations.json`, `ads.json`, `chat.html` for ChatGPT;
  `users.json` and `login_history.json` for Claude. Deliberately skipped, and said out loud.
- **`source.variant_version`** — the generation of the vendor's export shape a document was made
  from, as an integer, alongside the existing `variant` name. A third party reading the JSON can ask
  `variant_version >= 2` instead of matching strings, and each number is written up in the per-source
  log. `Detection` carries it too, and `panchat --inspect` prints it as `shape: vN`.
- **`ExportFile::reference`** — a file whose presence and size are known but whose bytes were not
  read, plus `size_bytes` and `loaded` fields on `ExportFile`.

### Changed

- **`read_path` no longer loads attachment blobs.** Structured files (`.json`, `.jsonl`, `.ndjson`,
  `.md`, `.txt`, and extensionless files up to 4 MiB) are read; everything else is referenced by
  name and size. Reading the 619 MB ChatGPT export dropped from 779 MB to 288 MB peak memory. A
  path naming a single file is still read whole.
- **SPEC: `ContentPart.Attachment.path` is no longer bundle-only.** It may be populated by a
  producer reading an unpacked export whose bytes are present, in which case it is relative to the
  export root and means nothing away from it. The schema description follows. Absence of `path`
  still means the bytes are unavailable and still requires `attachment_not_included`.
- **The CLI prints `error: <message>` and exits 1** rather than the debug form of the error type.
- **Variant names are one numbered series per vendor.** `official_export_v1_sharded` and
  `official_export_v2_directories` — both introduced unreleased — are now `official_export_v2` for
  ChatGPT and Claude respectively. Shape detection no longer keys off sharding or directories alone:
  a small 2026 ChatGPT account gets one `conversations.json` in the v2 layout, and a Claude account
  with no projects still gets the v2 `memories.json`.
- **SPEC: `Source` gained `variant_version`,** with the rule that a producer recognising more than
  one shape of a vendor's export SHOULD number them from 1 and publish what each number means.
  Numbers are per `platform` and per producer; consumers MUST NOT reject one they do not know.

### Fixed

- **ChatGPT: only the first conversations file was read.** The filename heuristic also matched
  `conversation_asset_file_names.json`, and nothing read past one file: a 1,285-conversation export
  imported as 100 conversations, with no warning.
- **Claude: the newer `memories.json` imported as nothing.** Parsing required a row `uuid` that the
  new shape does not have, so every memory was dropped silently.

### Notes for consumers

- `source.variant` gained the value `official_export_v2`, for both vendors, and every document now
  carries `source.variant_version`. Read the version rather than the string where you can.
- `ExportFile` gained public fields. Construct with `ExportFile::new` or `ExportFile::reference`.

## 0.1.0 — 2026-08-20

Initial release: the interchange format ([SPEC.md](SPEC.md), `schema/chat-v0.1.json`), the
ChatGPT and Claude adapters, structured lossiness warnings, and the `panchat` CLI.
