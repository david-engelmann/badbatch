"""Building, printing, and parsing of stamp documents."""

import re
from dataclasses import dataclass

from .errors import StampError
from .records import ACTIONS, CLASSES, UNCLASSED, check_path, content_digest

DIGEST = re.compile(r"[0-9a-f]{64}")


def class_word_refusal(source, owner, word):
    """The refusal for a class word outside the three named classes.

    The walk raises this same refusal for an unclassed artifact that
    no trusted judgment proposes a class for, so the recall refuses
    that artifact with the exact line the parser gives here.
    """
    return StampError(
        f"{source} is malformed, {owner} names the class "
        f"{word!r}, which is not one of the three named classes"
    )


@dataclass(frozen=True)
class StampFile:
    path: str
    action: str
    digest: object = None


@dataclass(frozen=True)
class StampArtifact:
    path: str
    rev_class: str
    digest: str
    parents: tuple


@dataclass(frozen=True)
class Stamp:
    source: str
    run: str
    agent: str
    prompt_hash: str
    model: str
    files: tuple
    artifacts: tuple

    def file_paths(self):
        return {entry.path for entry in self.files}

    def artifact_by_path(self):
        return {entry.path: entry for entry in self.artifacts}


def build_stamp(record):
    """The stamp for one parsed run record, sorted and digested."""
    files = tuple(
        StampFile(
            entry.path,
            entry.action,
            None if entry.action == "deleted" else content_digest(entry.content),
        )
        for entry in sorted(record.files, key=lambda entry: entry.path)
    )
    artifacts = tuple(
        StampArtifact(
            entry.path,
            entry.rev_class,
            content_digest(entry.content),
            tuple(sorted(entry.parents)),
        )
        for entry in sorted(record.artifacts, key=lambda entry: entry.path)
    )
    return Stamp(
        source=record.source,
        run=record.run,
        agent=record.agent,
        prompt_hash=record.prompt_hash,
        model=record.model,
        files=files,
        artifacts=artifacts,
    )


def format_stamp(stamp):
    """Render a stamp as the deterministic document text."""
    lines = [
        f"stamp {stamp.run}",
        f"agent {stamp.agent}",
        f"prompt-hash {stamp.prompt_hash}",
        f"model {stamp.model}",
    ]
    for entry in stamp.files:
        digest = entry.digest if entry.digest is not None else "none"
        lines.append(f"file {entry.path} {entry.action} {digest}")
    for entry in stamp.artifacts:
        parents = ", ".join(entry.parents)
        lines.append(
            f"artifact {entry.path} {entry.rev_class} {entry.digest} "
            f"derived-from {parents}"
        )
    return "\n".join(lines) + "\n"


def _digest_field(value, source, owner, allow_none):
    if value == "none" and allow_none:
        return None
    if not DIGEST.fullmatch(value):
        raise StampError(
            f"{source} is malformed, {owner} carries the digest "
            f"{value!r}, which is not sixty-four lowercase hex characters"
        )
    return value


def parse_stamp(text, source):
    """Parse one stamp document or raise StampError."""
    meaningful = []
    for raw_line in text.split("\n"):
        stripped = raw_line.strip()
        if stripped == "" or stripped.startswith("#"):
            continue
        meaningful.append(stripped)
    cursor = 0

    def take():
        nonlocal cursor
        if cursor >= len(meaningful):
            return None
        line = meaningful[cursor]
        cursor += 1
        return line

    first = take()
    if first is None or len(first.split()) != 2 or first.split()[0] != "stamp":
        raise StampError(
            f"{source} is malformed, it does not open with a stamp line "
            "naming the run"
        )
    run = first.split()[1]
    header = {}
    for field in ("agent", "prompt-hash", "model"):
        line = take()
        if line is None or len(line.split()) != 2 or line.split()[0] != field:
            raise StampError(
                f"{source} is malformed, the stamp is missing its "
                f"{field} field"
            )
        header[field] = line.split()[1]
    files = []
    artifacts = []
    seen = set()
    while True:
        line = take()
        if line is None:
            break
        parts = line.split()
        if parts[0] == "file" and len(parts) == 4:
            _, path, action, digest_text = parts
            check_path(path, source, StampError)
            if action not in ACTIONS:
                raise StampError(
                    f"{source} is malformed, the file line for {path} "
                    f"names the action {action!r}"
                )
            if path in seen:
                raise StampError(
                    f"{source} is malformed, the path {path} is "
                    "stamped twice"
                )
            seen.add(path)
            digest = _digest_field(
                digest_text, source, f"the file line for {path}",
                allow_none=action == "deleted",
            )
            files.append(StampFile(path, action, digest))
        elif parts[0] == "artifact" and len(parts) >= 6 and parts[4] == "derived-from":
            path = parts[1]
            check_path(path, source, StampError)
            owner = f"the artifact line for {path}"
            rev_class = parts[2]
            if rev_class not in CLASSES and rev_class != UNCLASSED:
                raise class_word_refusal(source, owner, rev_class)
            if path in seen:
                raise StampError(
                    f"{source} is malformed, the path {path} is "
                    "stamped twice"
                )
            seen.add(path)
            digest = _digest_field(parts[3], source, owner, allow_none=False)
            joined = " ".join(parts[5:])
            parents = tuple(part.strip() for part in joined.split(","))
            if any(parent == "" for parent in parents):
                raise StampError(
                    f"{source} is malformed, {owner} has an empty "
                    "derivation list"
                )
            for parent in parents:
                check_path(parent, source, StampError)
            artifacts.append(StampArtifact(path, rev_class, digest, parents))
        else:
            raise StampError(
                f"{source} is malformed, the line {line!r} matches no "
                "stamp line form"
            )
    for entry in artifacts:
        for parent in entry.parents:
            if parent not in seen:
                raise StampError(
                    f"{source} is malformed, the artifact {entry.path} "
                    f"derives from {parent}, which the stamp does not "
                    "carry"
                )
    return Stamp(
        source=source,
        run=run,
        agent=header["agent"],
        prompt_hash=header["prompt-hash"],
        model=header["model"],
        files=tuple(files),
        artifacts=tuple(artifacts),
    )
