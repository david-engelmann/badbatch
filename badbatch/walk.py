"""The recall walk over a stamp graph against a recorded current state."""

import heapq
from dataclasses import dataclass

from .errors import StateError, WalkError
from .judgment import check_answer, classification_request, trusted
from .records import CLASSES, UNCLASSED, check_path
from .stamps import DIGEST, class_word_refusal

CLASS_ORDER = {name: index for index, name in enumerate(CLASSES)}


@dataclass(frozen=True)
class WalkEntry:
    path: str
    rev_class: str
    status: str
    chain: tuple
    proposed: bool = False


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


def recall_walk(stamp, state, start, player=None):
    """Every downstream artifact of one stamped file, sorted for report.

    The walk follows derivation links from the start file through
    artifacts, keeps the shortest chain to each artifact with ties
    broken by the lexicographically smallest chain, and reports only.

    An artifact the stamp leaves unclassed is asked about once, when
    a judgment source is given, and a trusted answer proposes its
    class for this result alone. Without a trusted proposal the walk
    refuses that artifact with the stamp parser's class refusal, so
    the recall behaves exactly as it behaved before the seam existed.
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
    for path in sorted(best):
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
        rev_class = artifact.rev_class
        proposed = False
        if rev_class == UNCLASSED:
            answer = None
            if player is not None:
                request = classification_request(artifact, status)
                answer = check_answer(player.judge(request))
            if not trusted(answer):
                raise class_word_refusal(
                    stamp.source,
                    f"the artifact line for {path}",
                    UNCLASSED,
                )
            rev_class = answer["choice"]
            proposed = True
        entries.append(WalkEntry(path, rev_class, status, best[path], proposed))
    entries.sort(key=lambda entry: (CLASS_ORDER[entry.rev_class], entry.path))
    return tuple(entries)


def format_recall(start, entries):
    """Render a recall result as the deterministic document text."""
    lines = [f"recall {start}"]
    for entry in entries:
        chain = " -> ".join(entry.chain)
        marker = " proposed" if entry.proposed else ""
        lines.append(
            f"artifact {entry.path} {entry.rev_class}{marker} "
            f"{entry.status} chain {chain}"
        )
    return "\n".join(lines) + "\n"
