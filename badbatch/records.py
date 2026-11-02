"""Parsing of recorded run records, the text one run leaves about its work."""

import hashlib
from dataclasses import dataclass

from .errors import RecordError

ACTIONS = ("created", "modified", "deleted")
CLASSES = ("reversible", "reversible-with-compensation", "irreversible")
UNCLASSED = "unclassed"
HEADER_FIELDS = ("run", "agent", "prompt-hash", "model")


def content_digest(lines):
    """The digest of recorded content, SHA-256 over one newline per line."""
    digest = hashlib.sha256()
    for line in lines:
        digest.update(line.encode("utf-8"))
        digest.update(b"\n")
    return digest.hexdigest()


@dataclass(frozen=True)
class FileEntry:
    path: str
    action: str
    content: object = None


@dataclass(frozen=True)
class ArtifactEntry:
    path: str
    parents: tuple
    rev_class: str
    content: tuple = ()


@dataclass(frozen=True)
class RunRecord:
    source: str
    run: str
    agent: str
    prompt_hash: str
    model: str
    files: tuple
    artifacts: tuple


def check_path(path, source, error=RecordError):
    """Refuse a path the format cannot carry, naming the defect."""
    if not path or path.startswith("/") or ".." in path.split("/"):
        raise error(
            f"{source} is malformed, the path {path!r} is absolute, "
            "empty, or carries a parent reference"
        )


def _meaningful(raw, index):
    """The next meaningful line at or after index, as (index, stripped)."""
    while index < len(raw):
        stripped = raw[index].strip()
        if stripped == "" or stripped.startswith("#"):
            index += 1
            continue
        return index, stripped
    return index, None


def _content_block(raw, index, source, owner):
    """Consume the content block that must follow an entry line.

    The block opens at the next meaningful line, which must be the word
    content, and closes at the first later line that reads exactly end.
    Every line between is literal content.
    """
    open_index, line = _meaningful(raw, index)
    if line is None or line != "content":
        raise RecordError(
            f"{source} is malformed, {owner} is not followed by a "
            "content block"
        )
    lines = []
    cursor = open_index + 1
    while cursor < len(raw):
        if raw[cursor] == "end":
            return tuple(lines), cursor + 1
        lines.append(raw[cursor])
        cursor += 1
    raise RecordError(
        f"{source} is malformed, the content block for {owner} reaches "
        "the end of the record with no closing end line"
    )


def _parents_from_tokens(tokens, source, owner):
    joined = " ".join(tokens)
    parents = [part.strip() for part in joined.split(",")]
    if not joined or any(parent == "" for parent in parents):
        raise RecordError(
            f"{source} is malformed, {owner} has an empty derivation list"
        )
    return tuple(parents)


def parse_run_record(text, source):
    """Parse one recorded run record or raise RecordError."""
    raw = text.split("\n")
    index = 0
    header = {}
    for field in HEADER_FIELDS:
        index, line = _meaningful(raw, index)
        if line is None:
            raise RecordError(
                f"{source} is malformed, the header names no {field}"
            )
        parts = line.split()
        if len(parts) != 2 or parts[0] != field:
            raise RecordError(
                f"{source} is malformed, the header line {line!r} does "
                f"not name the {field} exactly once"
            )
        header[field] = parts[1]
        index += 1
    files = []
    artifacts = []
    while True:
        index, line = _meaningful(raw, index)
        if line is None:
            break
        parts = line.split()
        if parts[0] == "file" and len(parts) == 3:
            _, path, action = parts
            check_path(path, source)
            if action not in ACTIONS:
                raise RecordError(
                    f"{source} is malformed, the file entry for {path} "
                    f"names the action {action!r}, which is not created, "
                    "modified, or deleted"
                )
            index += 1
            if action == "deleted":
                _, peek = _meaningful(raw, index)
                if peek == "content":
                    raise RecordError(
                        f"{source} is malformed, the deleted file entry "
                        f"for {path} carries a content block"
                    )
                files.append(FileEntry(path, action, None))
            else:
                content, index = _content_block(
                    raw, index, source, f"the file entry for {path}"
                )
                files.append(FileEntry(path, action, content))
        elif parts[0] == "artifact" and len(parts) >= 5 and parts[2] == "derived-from":
            path = parts[1]
            check_path(path, source)
            owner = f"the artifact entry for {path}"
            if parts[-2] != "class":
                raise RecordError(
                    f"{source} is malformed, {owner} names no class"
                )
            rev_class = parts[-1]
            if rev_class not in CLASSES and rev_class != UNCLASSED:
                raise RecordError(
                    f"{source} is malformed, {owner} names the class "
                    f"{rev_class!r}, which is not one of the three "
                    "named classes"
                )
            parents = _parents_from_tokens(parts[3:-2], source, owner)
            for parent in parents:
                check_path(parent, source)
            index += 1
            content, index = _content_block(raw, index, source, owner)
            artifacts.append(ArtifactEntry(path, parents, rev_class, content))
        else:
            raise RecordError(
                f"{source} is malformed, the line {line!r} matches no "
                "entry form"
            )
    _check_graph(files, artifacts, source)
    return RunRecord(
        source=source,
        run=header["run"],
        agent=header["agent"],
        prompt_hash=header["prompt-hash"],
        model=header["model"],
        files=tuple(files),
        artifacts=tuple(artifacts),
    )


def _check_graph(files, artifacts, source):
    file_paths = [entry.path for entry in files]
    if len(set(file_paths)) != len(file_paths):
        dup = next(p for p in file_paths if file_paths.count(p) > 1)
        raise RecordError(
            f"{source} is malformed, the file path {dup} is entered twice"
        )
    artifact_paths = [entry.path for entry in artifacts]
    if len(set(artifact_paths)) != len(artifact_paths):
        dup = next(p for p in artifact_paths if artifact_paths.count(p) > 1)
        raise RecordError(
            f"{source} is malformed, the artifact path {dup} is "
            "entered twice"
        )
    known = set(file_paths) | set(artifact_paths)
    for entry in artifacts:
        if entry.path in file_paths:
            raise RecordError(
                f"{source} is malformed, the artifact path {entry.path} "
                "is also a file path"
            )
        for parent in entry.parents:
            if parent not in known:
                raise RecordError(
                    f"{source} is malformed, the artifact {entry.path} "
                    f"derives from {parent}, which the record never "
                    "declares as a file or as an artifact"
                )
    artifact_set = set(artifact_paths)
    parents_of = {
        entry.path: [p for p in entry.parents if p in artifact_set]
        for entry in artifacts
    }
    state = {}
    for entry in artifacts:
        if entry.path in state:
            continue
        stack = [(entry.path, [entry.path])]
        while stack:
            node, trail = stack[-1]
            if state.get(node) == "open":
                state[node] = "done"
                stack.pop()
                continue
            state[node] = "open"
            descended = False
            for parent in parents_of[node]:
                if state.get(parent) == "open":
                    raise RecordError(
                        f"{source} is malformed, the artifact {parent} "
                        "derives from itself through a chain of "
                        f"artifacts, {' then '.join(trail + [parent])}"
                    )
                if parent not in state:
                    stack.append((parent, trail + [parent]))
                    descended = True
            if not descended:
                state[node] = "done"
                stack.pop()
