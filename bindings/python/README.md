# panchat for Python

Read AI chat exports from any vendor into one representation — and say out loud what each export
left behind.

```bash
pip install panchat
```

```python
import panchat

doc = panchat.load("~/Downloads/chatgpt-export.zip")   # a zip, a folder, or one file

for conversation in doc["conversations"]:
    print(conversation.get("title", "(untitled)"))
    for message in panchat.active_messages(conversation):
        print(" ", message["role"], panchat.text(message)[:80])

for w in doc.get("warnings", []):
    print(w["severity"], w["code"], w.get("detail", ""))
```

The vendor is detected from the files; you never say which one it is. It reads ChatGPT and Claude
exports, Gemini from Google Takeout, and Claude Code's own session history.

## What you get back

Plain dicts and lists, shaped exactly as the
[JSON Schema](https://modelcaddy.github.io/panchat/schema/chat-v0.1.json) describes. There are no
wrapper classes, deliberately: the schema is the API, a document read here is byte for byte the one
the `panchat` command line prints, and keys this version does not know — a vendor's `x-`
extensions, the untouched `raw` payload — come through intact.

Two things to know before you write a consumer, both from the
[specification](https://github.com/modelcaddy/panchat/blob/main/SPEC.md):

- **`messages` is a graph, not a transcript.** Every regenerated answer and every edited prompt is
  still there as a sibling. Iterating `messages` gives you all of them interleaved. Use
  `panchat.active_messages(conversation)` for the thread the user last saw, and
  `panchat.off_path_messages(conversation)` for the rest.
- **`warnings` is the point.** Every loss is declared with a stable `code` and a `severity` —
  `info` (never existed in the export), `lossy` (existed, not fully represented), `dropped`. Show
  them to your users rather than rendering a broken image for an attachment the export never
  included.

## Functions

| | |
|---|---|
| `load(path)` | read an export from disk |
| `load_bytes(files)` | an export already in memory: `{"export.zip": data}` or `(path, bytes)` pairs |
| `detect(path)` | platform, export shape and confidence, without parsing everything |
| `render(doc, format)` | `markdown`, `jsonl`, or `turns` — one `{role, content}` per line for eval and fine-tuning, lossy by construction |
| `active_messages(c)`, `off_path_messages(c)`, `text(m)` | the three helpers every consumer ends up writing |

Errors are `panchat.NotRecognized` (nothing here is an export this version reads) and
`panchat.MalformedExport` (it is, and it cannot be read), both subclasses of `panchat.PanchatError`.
An export that *mostly* reads does not raise: one bad conversation is a warning, never the loss of
the other 9,999.

Parsing releases the GIL.

## Please do not attach your export to anything

If an export stops reading, open an issue with the output of `panchat.detect(path)` and a file
listing. Your export contains everything you have ever typed into that product.

Apache-2.0. Built on the [panchat](https://crates.io/crates/panchat) Rust crate.
