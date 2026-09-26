"""The Python binding, tested for the same properties as the Rust crate.

"It returned a dict" proves nothing. These assert that branches survive the
trip into Python, that losses are declared, that unknown keys come through, and
that a Python document is the Rust document.
"""

import io
import json
import pathlib
import threading
import zipfile

import pytest

import panchat

FIXTURES = pathlib.Path(__file__).resolve().parents[3] / "tests" / "fixtures"
CHATGPT = FIXTURES / "chatgpt_branched.json"
GEMINI = FIXTURES / "gemini_myactivity.json"


def zipped(entries):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        for name, data in entries.items():
            z.writestr(name, data)
    return buf.getvalue()


def test_version_and_format_are_exposed():
    assert panchat.FORMAT_VERSION == "0.1"
    assert panchat.SCHEMA_URL.endswith("chat-v0.1.json")
    assert panchat.__version__


def test_branches_survive_the_trip_into_python():
    doc = panchat.load(CHATGPT)
    c = doc["conversations"][0]
    active = panchat.active_messages(c)
    off = panchat.off_path_messages(c)
    assert off, "the regenerated answer is still there"
    assert len(active) + len(off) == len(c["messages"])
    assert all(m["id"] in c["active_path"] for m in active)


def test_losses_are_declared():
    doc = panchat.load(CHATGPT)
    codes = {w["code"] for w in doc["warnings"]}
    assert "attachment_not_included" in codes


def test_a_python_document_is_the_rust_document():
    # The claim the README makes: byte for byte what the CLI prints. The
    # round trip through render() goes back through the Rust type.
    doc = panchat.load(CHATGPT)
    lines = panchat.render(doc, "jsonl").splitlines()
    assert [json.loads(l) for l in lines] == doc["conversations"]


def test_unknown_keys_come_through():
    doc = panchat.load(CHATGPT)
    doc["x-acme"] = {"kept": True}
    doc["conversations"][0]["x-acme"] = {"also": 1}
    again = json.loads(panchat.render(doc, "jsonl").splitlines()[0])
    assert again["x-acme"] == {"also": 1}


def test_a_path_object_and_a_string_both_work():
    assert panchat.load(CHATGPT) == panchat.load(str(CHATGPT))


def test_an_export_in_memory_reads_like_one_on_disk():
    data = CHATGPT.read_bytes()
    assert panchat.load_bytes({"conversations.json": data}) == panchat.load(CHATGPT)
    assert panchat.load_bytes([("conversations.json", data)]) == panchat.load(CHATGPT)


def test_a_zip_in_memory_is_opened():
    # The upload case: somebody posts the vendor's zip to a web app.
    archive = zipped({"export/conversations.json": CHATGPT.read_bytes()})
    doc = panchat.load_bytes({"export.zip": archive})
    assert doc["source"]["platform"] == "chatgpt"


def test_detect_names_the_platform_without_parsing_everything(tmp_path):
    d = panchat.detect(GEMINI)
    assert d["platform"] == "gemini" and d["variant_version"] == 1
    junk = tmp_path / "notes.txt"
    junk.write_text("not an export")
    assert panchat.detect(junk) is None


def test_errors_are_typed():
    with pytest.raises(panchat.NotRecognized):
        panchat.load_bytes({"notes.txt": b"not an export"})
    with pytest.raises(panchat.PanchatError):
        panchat.load_bytes({"notes.txt": b"not an export"})
    assert issubclass(panchat.MalformedExport, panchat.PanchatError)


def test_a_missing_path_is_an_os_error(tmp_path):
    with pytest.raises(OSError):
        panchat.load(tmp_path / "nowhere")


def test_render_formats():
    doc = panchat.load(CHATGPT)
    assert panchat.render(doc).startswith("---")
    turn = json.loads(panchat.render(doc, "turns").splitlines()[0])
    assert set(turn) == {"conversation_id", "role", "content"}
    assert "off-path" in panchat.render(doc, "markdown", all_branches=True)
    with pytest.raises(ValueError):
        panchat.render(doc, "pdf")


def test_text_joins_only_text_parts():
    m = {"content": [{"type": "text", "text": "a"}, {"type": "attachment"}, {"type": "text", "text": "b"}]}
    assert panchat.text(m) == "a\nb"


def test_parsing_releases_the_gil():
    # Two loads on two threads must both finish; a held GIL would still finish,
    # so this guards against deadlock rather than proving concurrency.
    out = []
    ts = [threading.Thread(target=lambda: out.append(panchat.load(CHATGPT))) for _ in range(4)]
    for t in ts:
        t.start()
    for t in ts:
        t.join(timeout=30)
    assert len(out) == 4
