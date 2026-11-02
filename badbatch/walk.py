"""The recall walk over a stamp graph against a recorded current state."""

import heapq
from dataclasses import dataclass

from .errors import StateError, WalkError
from .records import CLASSES, check_path
from .stamps import DIGEST

CLASS_ORDER = {name: index for index, name in enumerate(CLASSES)}


@dataclass(frozen=True)
class WalkEntry:
    path: str
    rev_class: str
    status: str
    chain: tuple


def parse_current_state(text, source):
    """Parse one recorded current state into a path to digest mapping."""
    state = {}
    for raw_line in text.split("\n"):
        stripped = raw_line.strip()
        if stripped == "" or stripped.startswith("#"):
            continue
        parts = stripped.split()
        if len(parts) != 3 or parts[0] != "file":
            raise StateError(
                f"{source} is malformed, the line {stripped!r} matches "
                "no current state line form"
            )
        _, path, digest = parts
        check_path(path, source, StateError)
        if not DIGEST.fullmatch(digest):
            raise StateError(
                f"{source} is malformed, the line for {path} carries "
                f"the digest {digest!r}, which is not sixty-four "
                "lowercase hex characters"
            )
        if path in state:
            raise StateError(
                f"{source} is malformed, the path {path} is stated twice"
            )
        state[path] = digest
    return state


def recall_walk(stamp, state, start):
    """Every downstream artifact of one stamped file, sorted for report.

    The walk follows derivation links from the start file through
    artifacts, keeps the shortest chain to each artifact with ties
    broken by the lexicographically smallest chain, and reports only.
    """
    if start not in stamp.file_paths():
        raise WalkError(
            f"the recall cannot start from {start}, which is not a "
            "file entry in the stamp"
        )
    children = {}
    for artifact in stamp.artifacts:
        for parent in artifact.parents:
            children.setdefault(parent, []).append(artifact.path)
    best = {start: (start,)}
    heap = [(1, (start,), start)]
    while heap:
        _, chain, node = heapq.heappop(heap)
        if best.get(node) != chain:
            continue
        for child in sorted(children.get(node, [])):
            candidate = chain + (child,)
            known = best.get(child)
            if known is None or (len(candidate), candidate) < (len(known), known):
                best[child] = candidate
                heapq.heappush(heap, (len(candidate), candidate, child))
    by_path = stamp.artifact_by_path()
    entries = []
    for path, chain in best.items():
        if path == start:
            continue
        artifact = by_path[path]
        current = state.get(path)
        if current is None:
            status = "missing"
        elif current == artifact.digest:
            status = "current"
        else:
            status = "drifted"
        entries.append(WalkEntry(path, artifact.rev_class, status, chain))
    entries.sort(key=lambda entry: (CLASS_ORDER[entry.rev_class], entry.path))
    return tuple(entries)


def format_recall(start, entries):
    """Render a recall result as the deterministic document text."""
    lines = [f"recall {start}"]
    for entry in entries:
        chain = " -> ".join(entry.chain)
        lines.append(
            f"artifact {entry.path} {entry.rev_class} {entry.status} "
            f"chain {chain}"
        )
    return "\n".join(lines) + "\n"
