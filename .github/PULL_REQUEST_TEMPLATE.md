<!--
Thank you. If this adds or changes an adapter, the checklist below is what review
looks at — it is the same list as CONTRIBUTING.md, repeated here so it is in front
of you rather than in another file.

Delete whatever does not apply.
-->

## What this changes, and why

<!-- The diff says what. This paragraph says why. -->

## If this touches an adapter

- [ ] `detect` identifies the export by its **shape**, not only its filename, and does not claim
      another vendor's files.
- [ ] Unrecognised content survives — `ContentPart::Unknown`, `x`, or `raw`. Nothing is dropped or
      replaced with a placeholder.
- [ ] A malformed *item* produces a warning, not an `Err`. One bad conversation does not cost the
      user the rest.
- [ ] Everything the format cannot express is declared: a warning code, plus the README's lossiness
      table.
- [ ] Fixture tests assert the **properties** — branches preserved, warnings fired, unknown parts
      round-tripped — not merely that parsing succeeded.
- [ ] Every fixture is synthetic. No real conversation content, ids, filenames, or emails.
- [ ] `docs/formats/<platform>.md` records what the export looked like when you read it.
- [ ] CHANGELOG entry added.

## Checks

- [ ] `cargo fmt`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all-features` **and** `cargo test` — features gate behaviour here, so a `#[cfg]`
      mistake is invisible to `--all-features` alone.
