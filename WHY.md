# Why this is open, and what would help

*Source for the project's landing page. Written to be read by someone who has never seen the code.*

---

## The short version

Millions of people have years of their thinking inside four or five chat products. Every one of
those products offers an export. Not one of them tells you what the export leaves out.

**panchat reads any of those exports into one shape, and says out loud what each one threw away.**
It is Apache-2.0, it is free, and it always will be.

---

## The problem, concretely

Ask ChatGPT for your data and you get a file whose conversations are not lists of messages. They
are graphs. Every time you regenerated an answer, every time you edited a prompt and re-ran it, the
old branch is still in there as a sibling. Almost every importer ever written walks the current
branch and throws the rest away, silently and irreversibly, because a list is easier than a graph.

Ask Google for your Gemini history and you do not get conversations at all. You get *My Activity* —
the same log that records a search you ran or a video you watched — filtered to one product. No
conversation object, no title, no id, no thread. The most widely used open-source Google Takeout
library reads those files without complaining and drops every single answer, because its generic
activity parser has no branch for the field the answers live in.

Ask Claude and the export is clean and complete, and does not record which model wrote any given
answer — which is frequently the reason you kept the conversation.

None of this is documented. Vendors change these formats without announcing it. We know, because we
shipped a parser that read 100 conversations out of an export containing 1,285 and reported no
error at all. The bug was a filename heuristic that had been correct three months earlier.

**The failure mode in this category is not a crash. It is a silent, confident, partial answer.**
That is what this project is organised against.

---

## What it does about it

Three things, and the third is the one nobody else does.

**It keeps the graph.** Every message, including the branches you regenerated away, with a record
of which one you last saw. Nothing is flattened at parse time; flattening is a rendering decision
and it belongs where rendering happens.

**It never drops what it does not understand.** A message part it cannot interpret is carried
through verbatim rather than replaced with a placeholder. The vendor's original payload stays
attached to every conversation. An out-of-date parser degrades; it does not destroy.

**It publishes the losses.** Every parse returns structured warnings — this attachment's bytes are
not in your download, this format records no model, these 188 records are not conversations and
were not read. And the per-vendor table of what each export throws away is in the README, because
**no vendor publishes one**, and a person deciding where to keep the next four years of their
thinking deserves to know.

There is a specification and a JSON Schema, checked in CI. Every platform produces the same
document; only one field says which vendor it came from.

---

## Why open source

Three honest reasons, in the order they actually matter.

**Because a claim about your data is worthless in a closed binary.** This code reads every word you
have ever typed into a chat product. "It stays on your machine" is not a statement you should take
on faith from a compiled artefact. Here, the whole read path is on the screen, and
`panchat my-export.zip --format json | jq` either does what we say or it does not.

**Because nobody can write these parsers alone.** Nobody has an account on every platform, a copy
of every export generation, or a Japanese Takeout download. There are maybe a dozen products worth
reading and each one changes on its own schedule. Two vendors is a tool. Enough vendors is
infrastructure, and infrastructure only ever gets built by more than one person.

**Because a format is being settled right now, and the hard parts are missing from it.** The Data
Transfer Initiative has published an AI conversation schema with a vendor implementing it, and
regulators are moving toward obliging portability APIs. The model on the table is flat: messages,
senders, timestamps. No branches. No attachments. No tool calls. No model identity. If the default
answer to "what is an AI conversation export" gets fixed as that, then everything the vendors
already record and already ship gets defined out of existence — not by anyone's decision, just by
nobody having written down that it was there.

The lossiness table is the argument. It only carries weight if anyone can check it.

---

## Who is behind it, and where the money is

Worth saying plainly, because the answer changes how you should read everything above.

panchat was extracted from **ModelCaddy**, a paid, closed-source desktop app that reads your chat
history and builds a memory you can carry into the next conversation. ModelCaddy depends on this
library exactly the way anyone else would.

The split is deliberate and it is not going to move:

| Open, and staying open | Paid |
|---|---|
| Reading your exports | Understanding them |
| Every vendor adapter | Extraction, synthesis, memory |
| The interchange format and its schema | The app around it |

Reading is commodity work with no defensibility, and every hour someone else spends fixing a
Gemini export change is an hour we do not spend. Making sense of what is inside is the product. We
would rather say that out loud than have you wonder.

Practically, this means the library is not going to be quietly abandoned when it stops being
strategic, because it is load-bearing for something that pays for itself. It also means you should
hold us to the boundary. If the free half ever starts getting worse to make the paid half look
better, that is a bug report worth filing loudly.

Apache-2.0 is a one-way door: what is released stays released, including for competitors, including
inside closed products. The only thing not licensed is the names, and only because if two different
programs answer to `panchat` then "your export was read by panchat" stops meaning anything.

---

## What would actually help

In order of how much difference it makes, which is not the order you would guess.

### 1. Tell us when your export stops reading

**This is the most valuable thing anyone can do, and it needs no code.** No vendor announces a
format change. The only way we find out is that somebody's import broke.

Run `panchat <your-export> --inspect`, open an issue, and paste what it printed along with what
looked wrong. It prints counts and a detected shape — not your conversations.

**Never attach your export.** It contains everything you have ever typed into that product, nobody
here wants it, and GitHub keeps it forever. The issue templates ask only for machine output and a
file listing for exactly this reason.

### 2. Tell us what your export looks like

We support three platforms. There are at least a dozen worth reading: Copilot, Grok, DeepSeek,
Perplexity, and the local tools — Open WebUI, LM Studio, Jan, SillyTavern — where the history is a
database or a directory rather than an export.

You do not have to write the adapter. Knowing a shape exists is most of the work. Open an issue with
the file listing and the key names — the names, not the values — and the four questions that decide
how much survives:

- Are regenerated or edited turns kept, or only the final thread?
- Is the model recorded per message?
- Do attachment bytes ship, or only references?
- Are there side-cars — projects, folders, memories, custom instructions?

"I don't know" is a fine answer to any of them.

The Gemini support that exists today was built without ever holding a Gemini export, by reading
twenty other people's parsers. Its format log lists six things still unconfirmed. One person with a
real export and ten minutes could close most of them.

### 3. Write an adapter

Four methods. One file, one line in the registry, a synthetic fixture, and a page recording what
the export looked like the day you read it.

The review bar is about honesty rather than polish: detect by shape and not by filename, preserve
what you do not understand, turn a malformed item into a warning instead of an error, and **declare
what you dropped**. An adapter that parses cleanly by staying quiet about its losses is worse than
no adapter at all.

`CONTRIBUTING.md` walks through it, and says how long you should expect to wait.

### 4. Build something that reads the output

A viewer, a search tool, an analysis pipeline, a thing that finally lets you grep four years of
your own thinking. This is the contribution that tells us where the format is awkward, which is not
knowable from the inside — and it is the reason to have a format at all.

If you are building one, say so in an issue. A format with two independent implementations is a
format. With one, it is a data structure with delusions.

---

## What we will not do

- **Write a vendor's export format back out.** No vendor imports its own export. It would be a
  format nobody reads.
- **Put derived data in the core.** No embeddings, no summaries, no extracted tasks. This carries
  what a vendor exported, not what a tool inferred from it.
- **Ask for more of your personal data than a conversation needs.** There is no user or account
  object in the format, on purpose.
- **Automate a vendor's web UI.** Fragile, and somebody else's terms of service.

---

## Start here

```bash
cargo install panchat --features cli
panchat ~/Downloads/your-export.zip --inspect
```

It will tell you what it found, and what your vendor left out.

- **Code and issues:** <https://github.com/modelcaddy/panchat>
- **What is planned, in order:** [ROADMAP.md](ROADMAP.md)
- **The format:** [SPEC.md](SPEC.md)
- **What each vendor throws away:** the table in [README.md](README.md)
