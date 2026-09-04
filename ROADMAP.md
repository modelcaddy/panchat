# Roadmap

What this project intends to do next, in the order it intends to do it, as of **2026-09-04**.
Every item is an open issue, so the discussion happens there and this page is only the index. The
order is a judgement about payoff per unit of work for the people who own the exports, and it
changes when a drift report or a contributor changes the facts.

The point of the project has not moved: read any vendor's export into one representation, keep
every branch, and say out loud what was lost. Everything below serves one of those three.

## Where it stands

Version 0.3.0 reads ChatGPT and Claude exports, unpacked or still zipped, keeps the branch graph,
and emits structured warnings. The interchange format is specified in [SPEC.md](SPEC.md) at 0.1
with its [JSON Schema](schema/chat-v0.1.json) checked in CI, and a CLI proves the claim in one
command. Two vendors is a tool. The work below is what turns it into infrastructure.

Three facts from outside the repository shape the order:

- **A portability format is being decided without the hard parts.** The Data Transfer Initiative
  has published an [AI Conversation History](https://schemas.pub/schemas/24) schema with Inflection
  AI, and the EU is weighing a Digital Markets Act designation for ChatGPT that would oblige a
  portability API. Their model is flat: no branches, attachments, tool calls or per-message model.
  The lossiness table in the README is the argument for including them, and it needs to reach that
  venue.
- **The consumers are in Python.** Researchers, the Open WebUI import scripts, and every
  "chat with my history" side project. A Rust crate is invisible to them.
- **The largest exports have already changed shape again.** A big ChatGPT account now receives a
  zip of zips, and the archive reader refuses nested archives by design.

## Now

Done, unreleased, and waiting on the one thing this repository cannot supply itself.

1. ~~[#13](https://github.com/modelcaddy/panchat/issues/13) — repository hygiene.~~ Done, with one
   change of plan: there is deliberately **no `NOTICE` file**. Apache-2.0 section 6 makes
   reproducing `NOTICE` content an express exception to the trademark non-grant, so a name defended
   there would be a name licensed away. [TRADEMARK.md](TRADEMARK.md) carries it instead, under
   section 4(c).
2. ~~[#5](https://github.com/modelcaddy/panchat/issues/5) — a zip of zips.~~ One bounded level of
   nesting is followed, decided by shape rather than by filename. **Still open on evidence:** the
   layout is reported rather than observed, and a multi-gigabyte attachment part still exceeds the
   read budget until streaming lands.
3. ~~[#6](https://github.com/modelcaddy/panchat/issues/6) — Gemini, from Google Takeout.~~ Shipped,
   and built without anyone here ever holding a Gemini export — it is reconstructed from twenty
   other parsers, and [its format log](docs/formats/gemini.md) lists six things still unconfirmed.

**What is blocking all three is the same thing: nobody here has the export.** If you have a large
ChatGPT download or any Gemini Takeout, the `--inspect` output and a file listing would move more
than any amount of further code. Never the export itself.

## Next

The release after, where the library becomes usable by people who do not write Rust.

4. [#7](https://github.com/modelcaddy/panchat/issues/7) — Python binding, `pip install panchat`,
   wheels from CI. The single item most likely to decide whether anyone else uses this.
5. [#8](https://github.com/modelcaddy/panchat/issues/8) — Open the adapter registry so an adapter
   can be maintained outside this repository. Includes the `Detection` field change, which is
   cheap now and expensive after 1.0.
6. [#9](https://github.com/modelcaddy/panchat/issues/9) — Stream large exports. Memory bounded by
   the largest conversation, not the file.
7. [#10](https://github.com/modelcaddy/panchat/issues/10) — A capture producer. The specification
   defines `method: capture` and requires `branches_unavailable` from it, and nothing in the tree
   emits either. Prove the rule here before a third party gets it wrong alone.

## Later

8. [#11](https://github.com/modelcaddy/panchat/issues/11) — Bridge to the Data Transfer
   Initiative schema: a lossy sink and an adapter, so this library is the pivot between vendor
   exports as they ship and the format regulators are converging on.
9. [#12](https://github.com/modelcaddy/panchat/issues/12) — Publish the lossiness matrix as a
   page on the existing Pages site, generated from `docs/formats/`, with a drift log.
10. More adapters, in the order people ask: Grok, Copilot, Perplexity, and the local tools
    (Open WebUI, LM Studio, Jan, SillyTavern) whose history lives in a database or a directory
    rather than an export. Each arrives as its own issue under the `adapter` label.
11. WASM and npm, for a viewer that runs in a browser tab and never uploads the export, and a
    single-file reference viewer of a couple of hundred lines. A "build your own viewer" claim
    without an example is marketing copy.

## Toward 1.0

1.0 is a compatibility promise, not a milestone, and it is earned by external implementers hitting
the shape rather than by time passing. Before it:

- The four open questions at the end of [SPEC.md](SPEC.md): whether `active_path` becomes
  required, inline attachment bytes, threading across conversations, and artifact modelling.
- The `.chat` bundle form, which is specified and not implemented. Implement it or remove it from
  the specification; a form nobody has written is not a form.
- Revising the specification against what at least one producer and one consumer outside this
  repository actually hit, then publishing it under CC-BY 4.0.
- Registering a media type, so the format can be cited rather than described.

## Not planned

Stated so nobody waits for them.

- Writing a vendor's own export format back out. No vendor imports its own export.
- An N-to-N converter. N adapters into one representation and a few sinks out is the whole design.
- Derived data in the core namespace: embeddings, summaries, extracted tasks. That belongs under
  an `x-` key, produced by whatever tool derived it.
- A user or account object in the format. Exports contain enough personal data already.

## How this list changes

A format-drift report moves its platform to the top of *Now*, because an export that has stopped
reading is worth more than any feature. A contributor who wants to write an adapter can take it out
of order; open the issue and say so. Anything else is a pull request against this file, with the
reason in the description.
