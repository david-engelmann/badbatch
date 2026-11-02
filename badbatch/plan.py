"""The execution plan for a recall, built over the recall walk.

The plan is a document a human reviewer reads before any recall is
executed. It states, for every artifact the walk lists, the
candidate compensations that could undo the artifact and the order
a reviewer should consider them in. The candidate set is a
deterministic function of the artifact class and status. A recorded
judgment may rank the candidates of one artifact by score, and
that ranking orders only the options the reviewer sees. The plan
executes nothing, approves nothing, and triggers nothing.
"""

from dataclasses import dataclass

from .judgment import (
    CANDIDATES,
    check_ranking_answer,
    ranking_request,
    trusted_score,
)
from .errors import WalkError
from .walk import recall_walk

_REBUILD, _REMOVE, _QUARANTINE = CANDIDATES


def candidates_for(rev_class, status):
    """The candidate compensations one walk entry carries, in order.

    The set is a function of the class and the status and of
    nothing else. A missing artifact carries no candidate. A
    drifted artifact is quarantined or removed and never rebuilt,
    because rebuilding would overwrite work the stamp never
    recorded. A current irreversible artifact is quarantined alone.
    """
    if status == "missing":
        return ()
    if status == "drifted":
        return (_QUARANTINE, _REMOVE)
    if rev_class == "reversible":
        return (_REBUILD, _REMOVE)
    if rev_class == "reversible-with-compensation":
        return CANDIDATES
    if rev_class == "irreversible":
        return (_QUARANTINE,)
    raise WalkError(
        f"no compensation candidates are defined for the class {rev_class}"
    )


@dataclass(frozen=True)
class PlannedCompensation:
    name: str
    score: float | None = None


@dataclass(frozen=True)
class PlanEntry:
    path: str
    rev_class: str
    status: str
    chain: tuple
    proposed: bool
    compensations: tuple


def _order_candidates(candidates, scores):
    """Place trusted scored candidates first, then the unscored.

    Scored candidates order by score from highest to lowest, and
    the sort is stable over the canonical order, so equal scores
    keep their canonical order among themselves. Candidates no
    trusted score covers follow in their canonical order.
    """
    scored = [name for name in candidates if name in scores]
    scored.sort(key=lambda name: -scores[name])
    unscored = [name for name in candidates if name not in scores]
    return scored + unscored


def build_plan(stamp, state, start, player=None):
    """The execution plan for one stamped file, in walk order.

    The plan runs the same walk the recall runs, so classification
    proposals and refusals behave exactly as they behave there.
    Where a judgment source is given, an artifact carrying two or
    more candidates is asked about once per candidate, and trusted
    scores order its candidates under the ranking rule. An artifact
    carrying fewer than two candidates is never asked about.
    """
    entries = recall_walk(stamp, state, start, player)
    planned = []
    for entry in entries:
        candidates = candidates_for(entry.rev_class, entry.status)
        scores = {}
        if player is not None and len(candidates) >= 2:
            for candidate in candidates:
                request = ranking_request(entry, candidate)
                answer = check_ranking_answer(player.judge_ranking(request))
                if trusted_score(answer):
                    value = float(answer["score"])
                    scores[candidate] = value if value != 0 else 0.0
        ordered = _order_candidates(candidates, scores)
        planned.append(
            PlanEntry(
                path=entry.path,
                rev_class=entry.rev_class,
                status=entry.status,
                chain=entry.chain,
                proposed=entry.proposed,
                compensations=tuple(
                    PlannedCompensation(name, scores.get(name))
                    for name in ordered
                ),
            )
        )
    return tuple(planned)


def format_plan(start, entries):
    """Render a plan as the deterministic document text."""
    lines = [f"plan {start}"]
    for entry in entries:
        chain = " -> ".join(entry.chain)
        marker = " proposed" if entry.proposed else ""
        lines.append(
            f"artifact {entry.path} {entry.rev_class}{marker} "
            f"{entry.status} chain {chain}"
        )
        if entry.status == "missing":
            lines.append(
                "compensation none required, the artifact is already absent"
            )
            continue
        for compensation in entry.compensations:
            if compensation.score is None:
                lines.append(f"compensation {compensation.name}")
            else:
                lines.append(
                    f"compensation {compensation.name} "
                    f"score {compensation.score!r}"
                )
    return "\n".join(lines) + "\n"
