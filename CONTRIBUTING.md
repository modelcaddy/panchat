# Contributing

The useful thing you can do here is **teach it to read one more export**. Three vendors is a tool;
enough vendors is infrastructure, and no one person has an account on every platform or a copy of
every export shape. That is the part of this project that only contributors can supply.

You do not need to write Rust to help. A report that a vendor changed its export is worth as much as
the patch that follows it, and it is the thing we cannot get any other way.

## What is most wanted

1. **A new platform.** Copilot, Grok, DeepSeek, Perplexity, and the local tools — Open WebUI,
   LM Studio, Jan, SillyTavern. See "Adding an adapter" below.
2. **A format-drift report.** Your export no longer parses, or parses wrongly. Open an issue with
   the output of `panchat <your-export> --inspect` and what looked wrong. Do not attach the export.
3. **A fixture.** A small, synthetic file capturing a shape we do not have. Read "Fixtures and your
   privacy" first — it is the section that matters most.
4. **A consumer.** Something that reads the output — a query tool, a viewer, a pipeline. It tells us
   where the format is awkward, which is not knowable from the inside.

## Adding an adapter

An adapter is one file in `src/adapters/`, one line in the registry, fixtures, and a format log.

**1. Implement the trait** (`src/adapters/mod.rs`):

```rust
pub trait Adapter: Send + Sync {
    fn platform(&self) -> &'static str;
    fn variant(&self) -> &'static str;
    fn detect(&self, files: &[ExportFile]) -> Option<Detection>;
    fn parse(&self, files: &[ExportFile], warnings: &mut Warnings) -> Result<Document, Error>;
}
```

An adapter is **pure with respect to its inputs**. It never touches the filesystem, a clock, a
network, or a database — it is handed the export's files and returns a `Document`. That is what
makes it testable from fixtures, and fixture tests are the only thing standing between this crate
and silent rot when a vendor changes something.

**2. Register it** in `adapters::all()`, in descending order of how distinctive its shape is.

**3. Write fixture tests** in `tests/adapters.rs`.

**4. Write a format log** — `docs/formats/<platform>.md`. Copy the structure of an existing one:
what the export looked like on the day you read it, its layout, what was in it, and what the adapter
does about each part. This is a record of *observation*, because no vendor documents its export.

**5. Add a CHANGELOG entry** and a row to the README's support table.

### What review will check

These are the rules that make the output worth trusting, so they are not negotiable:

- **`detect` must not claim another vendor's files.** ChatGPT and Claude both ship a
  `conversations.json`; the filename is not enough, so both adapters test the *shape* inside.
  `detect` should also be cheap — it runs for every adapter on every input, so parse the smallest
  thing that identifies the export, not the whole export.
- **Never drop what you do not understand.** An unrecognised content part becomes
  `ContentPart::Unknown` with the vendor's payload intact, not a placeholder. Anything you cannot
  model goes in `x` (namespaced) or `raw`. An out-of-date parser must degrade, never destroy.
- **A malformed item is a warning, not an error.** Return `Err` only when the input is not yours or
  is structurally unreadable. One bad conversation must never cost the user the other 9,999 — that
  is `WarningCode::ItemSkipped`.
- **Declare your losses.** If the export has something the IR cannot hold, emit a warning for it and
  add it to the README's lossiness table. The table is the reason this crate exists: no vendor
  publishes one. An adapter that parses cleanly by staying quiet about what it dropped is worse than
  no adapter.
- **Nothing platform-specific in the core namespace.** Vendor concepts go under `x-<platform>`.
- **Test the properties, not the parse.** "It returned Ok" proves nothing. Assert that branches
  survived, that the warning fired, that the unknown part round-tripped.

## Fixtures and your privacy

**Never attach a real export to an issue or a pull request.** Yours contains everything you have
ever typed into that product. Nobody here wants it and GitHub keeps it forever.

Every fixture in `tests/fixtures/` is synthetic and small — hand-written to capture one shape. That
is the standard for new ones too. To build one:

- **Keep** the structure: key names, nesting, types, null-vs-absent, the id *format*, and whatever
  edge case you are capturing (a branch, an empty part, a broken pointer).
- **Replace** everything else: message text, titles, real ids, filenames, emails, URLs, timestamps.
  Make the content obviously invented.
- **Trim** to the smallest thing that still reproduces the shape — usually two or three
  conversations, not two thousand.

If you cannot describe a shape without real data, describe it in words in an issue instead and we
will build a fixture together. That is a normal outcome, not a failure.

## Working on the code

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test
```

Run the last two separately and both, always. Features here gate *behaviour*, not just compilation —
the `zip` feature changes what a zip input does — so a mistake in a `#[cfg]` is invisible to
`--all-features` alone. CI runs exactly these.

Stable Rust, 2021 edition. No MSRV is promised yet.

Commit messages explain **why**, in prose. The diff already says what changed.

## Where this project will change under you

Being honest about the state of things is cheaper than being embarrassed later:

- **It is 0.x, and the interchange format is 0.1.** Both the Rust API and the emitted JSON can break
  in a minor release. If you build on it, pin the version and read `format_version` at runtime.
- **The adapter registry is closed.** `Adapter` is public, but `adapters::all()` is a hardcoded
  list, so an adapter maintained outside this repository will compile and never be selected by
  `normalize`. For now, adapters live in-tree and arrive as pull requests. If you want to maintain
  one out-of-tree, open an issue — that is a design change worth making, not a workaround.
- **Writing a vendor's own export format back out is not supported and will not be.** No vendor
  imports its own export, so a writer would be a format nobody reads.
- **Nothing streams yet.** A very large `conversations.json` is held in memory whole.

What is planned about each of these, and in what order, is in [ROADMAP.md](ROADMAP.md).

## What to expect back

This is maintained by one person, alongside other work, so the honest answer about timing is a
range rather than a promise:

- **A format-drift report** — "my export stopped reading" — is looked at first, because an export
  that no longer parses is worth more than any feature. Expect a reply within about a week.
- **A pull request adding an adapter** gets a real review, but it may sit for a couple of weeks
  before it gets one. Reviewing an adapter means reading it against the rules above, not skimming
  it, and that is not a thing to do in a spare ten minutes.
- **A pull request that rewrites the architecture** will probably be declined, and faster than the
  others. That is not a judgement of the code — [ROADMAP.md](ROADMAP.md) and the open issues are
  where a change of that size gets agreed before it gets written.
- **Silence is not a verdict.** If something has gone quiet for longer than the above, a comment
  saying so is welcome and is not a nuisance.

Merge rights stay with the maintainer. That is normal for a project this size and is not a
statement about anyone's patch.

## License

Apache-2.0. Contributions are accepted under the same license. There is no CLA: Apache-2.0 already
grants what this project needs, including from contributors, so nobody has to sign anything to
send a patch.

[TRADEMARK.md](TRADEMARK.md) covers the one thing the license does not, which is the names. It does
not restrict contributing, using, or forking — only shipping a modified version under this name.
