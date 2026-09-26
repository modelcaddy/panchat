"""Read AI chat exports from any vendor into one representation, with the losses named.

Every function returns plain Python values — dicts, lists, strings — shaped
exactly as the JSON Schema describes, because the schema is the API:

    https://modelcaddy.github.io/panchat/schema/chat-v0.1.json

A document read here is the document the ``panchat`` command line prints.
Nothing is converted into classes, so nothing can drift from the format, and
keys this version does not know about — a vendor's ``x-`` extensions, the
untouched ``raw`` payload — come through as they are.

    >>> import panchat
    >>> doc = panchat.load("chatgpt-export.zip")
    >>> for c in doc["conversations"]:
    ...     for m in panchat.active_messages(c):
    ...         print(m["role"], panchat.text(m))
    >>> for w in doc.get("warnings", []):
    ...     print(w["severity"], w["code"], w.get("detail", ""))
"""

from __future__ import annotations

import json
import os
from typing import Any, Dict, Iterable, List, Optional, Tuple, Union

from . import _panchat
from ._panchat import (
    FORMAT_VERSION,
    SCHEMA_URL,
    MalformedExport,
    NotRecognized,
    PanchatError,
    __version__,
)

__all__ = [
    "load",
    "load_bytes",
    "detect",
    "render",
    "active_messages",
    "off_path_messages",
    "text",
    "FORMAT_VERSION",
    "SCHEMA_URL",
    "PanchatError",
    "NotRecognized",
    "MalformedExport",
    "__version__",
]

Document = Dict[str, Any]
PathLike = Union[str, "os.PathLike[str]"]


def load(path: PathLike) -> Document:
    """Read an export — its folder, its zip, or a single file — into a document.

    The vendor is detected from the files, never passed in. Raises
    :class:`NotRecognized` when nothing looks like an export this version can
    read, and :class:`MalformedExport` when it does but cannot be read.

    Read the ``warnings`` key. It is the point: what the export left out, said
    out loud rather than silently.
    """
    return json.loads(_panchat.load(os.fspath(path)))


def load_bytes(files: Union[Iterable[Tuple[str, bytes]], Dict[str, bytes]]) -> Document:
    """Normalize an export already in memory.

    ``files`` is ``(relative_path, bytes)`` pairs, or a dict of them — an
    upload, an object store, a database row. A single zip passed this way is
    opened exactly as one read from disk would be::

        panchat.load_bytes({"export.zip": request.files["export"].read()})
    """
    pairs = files.items() if isinstance(files, dict) else files
    return json.loads(_panchat.normalize_files([(str(p), bytes(b)) for p, b in pairs]))


def detect(path: PathLike) -> Optional[Dict[str, Any]]:
    """Which platform and export shape this is, or ``None``.

    Returns ``platform``, ``variant``, ``variant_version``, ``confidence`` and
    ``notes``. Cheaper than :func:`load` for asking "can this be read?".
    """
    found = _panchat.detect(os.fspath(path))
    return None if found is None else json.loads(found)


def render(document: Document, format: str = "markdown", all_branches: bool = False) -> str:
    """Render a document as ``markdown``, ``jsonl`` or ``turns``.

    ``turns`` is one ``{conversation_id, role, content}`` per line, for eval and
    fine-tuning pipelines, and is lossy by construction. By default only the
    branch the user last saw is rendered; ``all_branches=True`` renders every
    regeneration too, marked.
    """
    return _panchat.render(json.dumps(document), format, all_branches)


def active_messages(conversation: Dict[str, Any]) -> List[Dict[str, Any]]:
    """The messages of the branch the user last saw, in order.

    Iterating ``conversation["messages"]`` directly is the commonest mistake a
    consumer makes: it is a graph flattened into a list, and it yields every
    regenerated answer interleaved with the kept one. Walk this instead.
    """
    messages = conversation.get("messages", [])
    path = conversation.get("active_path") or []
    if not path:
        return list(messages)
    by_id = {m["id"]: m for m in messages}
    return [by_id[i] for i in path if i in by_id]


def off_path_messages(conversation: Dict[str, Any]) -> List[Dict[str, Any]]:
    """Messages that exist but are not on the active branch — regenerations,
    edited-away prompts, abandoned branches."""
    path = set(conversation.get("active_path") or [])
    if not path:
        return []
    return [m for m in conversation.get("messages", []) if m["id"] not in path]


def text(message: Dict[str, Any]) -> str:
    """The message's text parts joined by newlines. Attachments, tool calls and
    parts this version does not model contribute nothing — inspect
    ``message["content"]`` for those."""
    return "\n".join(
        p["text"] for p in message.get("content", []) if p.get("type") == "text"
    )
