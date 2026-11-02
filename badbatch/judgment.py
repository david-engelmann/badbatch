"""Recorded decision model judgments, played back from fixtures.

The roadmap gives badbatch two decision points. At the proposed
classification point, where the stamp data bounds no class for an
artifact, a judgment may propose one of the three reversibility
classes for that artifact. At the compensation ranking point, a
judgment may score each candidate compensation for one artifact in
an execution plan, rating how completely it restores the prior
state. Both judgments are advisory. A proposal is marked as
proposed wherever the walk output shows it and never becomes the
recorded class, a ranking only orders the options a reviewer sees,
and no judgment executes, approves, or triggers a recall.

A judgment is recorded as a fixture that pairs one request with one
response in the shape a decision model returns, under the name of
its point. A choice response carries the chosen label, a
confidence, and a probability for every label. A score response
carries a score and a confidence. An abstention response carries
none of those fields. The fixture player answers a request only
from a recorded fixture for the same point whose request matches it
exactly, and abstains where no fixture matches. Everything here is
offline. The player reads files from one directory and nothing
else.
"""

import json
import math
import re
from pathlib import Path

from .errors import JudgmentError
from .records import CLASSES

CONFIDENCE_THRESHOLD = 0.6

JUDGMENT_POINT = "classification"
RANKING_POINT = "compensation-ranking"
CANDIDATES = ("rebuild-from-parents", "remove-artifact", "quarantine-artifact")

_DIGEST = re.compile(r"[0-9a-f]{64}")
_REQUEST_FIELDS = {"artifact", "parents", "digest", "status"}
_RANKING_REQUEST_FIELDS = {"artifact", "candidate", "class", "status", "chain"}
_STATUSES = ("current", "drifted", "missing")
_FIELDS_FOR_KIND = {
    JUDGMENT_POINT: {
        "abstention": {"kind"},
        "choice": {"kind", "choice", "confidence", "probabilities"},
    },
    RANKING_POINT: {
        "abstention": {"kind"},
        "score": {"kind", "score", "confidence"},
    },
}


def _fail(source, defect):
    raise JudgmentError(source + " " + defect)


def _is_number(value):
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        return False
    try:
        return math.isfinite(float(value))
    except OverflowError:
        return False


def _check_unit_interval(source, field, value):
    if not _is_number(value) or not 0.0 <= float(value) <= 1.0:
        _fail(source, "field '" + field + "' is not a number between 0 and 1")


def _validate_request(source, request):
    if set(request) != _REQUEST_FIELDS:
        _fail(
            source,
            "request does not carry exactly the fields artifact, "
            "parents, digest, and status",
        )
    artifact = request.get("artifact")
    if not isinstance(artifact, str) or not artifact:
        _fail(source, "request field 'artifact' is not a non-empty string")
    parents = request.get("parents")
    if (
        not isinstance(parents, list)
        or not parents
        or not all(isinstance(parent, str) and parent for parent in parents)
    ):
        _fail(
            source,
            "request field 'parents' is not a non-empty list of strings",
        )
    digest = request.get("digest")
    if not isinstance(digest, str) or not _DIGEST.fullmatch(digest):
        _fail(
            source,
            "request field 'digest' is not sixty-four lowercase "
            "hex characters",
        )
    if request.get("status") not in _STATUSES:
        _fail(
            source,
            "request field 'status' is not current, drifted, or missing",
        )


def _validate_ranking_request(source, request):
    if set(request) != _RANKING_REQUEST_FIELDS:
        _fail(
            source,
            "request does not carry exactly the fields artifact, "
            "candidate, class, status, and chain",
        )
    artifact = request.get("artifact")
    if not isinstance(artifact, str) or not artifact:
        _fail(source, "request field 'artifact' is not a non-empty string")
    if request.get("candidate") not in CANDIDATES:
        _fail(
            source,
            "request field 'candidate' is not one of the three "
            "candidate compensations",
        )
    if request.get("class") not in CLASSES:
        _fail(
            source,
            "request field 'class' is not one of the three "
            "reversibility classes",
        )
    if request.get("status") not in _STATUSES:
        _fail(
            source,
            "request field 'status' is not current, drifted, or missing",
        )
    chain = request.get("chain")
    if not isinstance(chain, str) or not chain:
        _fail(source, "request field 'chain' is not a non-empty string")


def _validate_response(source, response, point=JUDGMENT_POINT):
    if not isinstance(response, dict):
        _fail(source, "response is not an object")
    kind = response.get("kind")
    fields_for_kind = _FIELDS_FOR_KIND[point]
    if isinstance(kind, str) and kind in fields_for_kind:
        extra = set(response) - fields_for_kind[kind]
        if extra:
            _fail(
                source,
                "response carries the field "
                + repr(sorted(extra, key=repr)[0])
                + ", which a "
                + kind
                + " response does not carry",
            )
    if kind == "abstention":
        return
    if point == RANKING_POINT:
        if kind != "score":
            _fail(
                source,
                "response kind "
                + repr(kind)
                + " does not answer the compensation-ranking point, "
                "which takes a score or an abstention",
            )
        _check_unit_interval(source, "score", response.get("score"))
        _check_unit_interval(source, "confidence", response.get("confidence"))
        return
    if kind != "choice":
        _fail(
            source,
            "response kind "
            + repr(kind)
            + " does not answer the classification point, which "
            "takes a choice or an abstention",
        )
    _check_unit_interval(source, "confidence", response.get("confidence"))
    choice = response.get("choice")
    if choice not in CLASSES:
        _fail(
            source,
            "response field 'choice' is not one of the three "
            "reversibility classes",
        )
    probabilities = response.get("probabilities")
    if not isinstance(probabilities, dict):
        _fail(source, "response field 'probabilities' is not an object")
    if set(probabilities) != set(CLASSES):
        _fail(
            source,
            "response probabilities do not cover exactly the three "
            "reversibility classes",
        )
    total = 0.0
    for label in CLASSES:
        _check_unit_interval(source, "probabilities." + label, probabilities[label])
        total += float(probabilities[label])
    if abs(total - 1.0) > 0.001:
        _fail(source, "response probabilities do not add up to 1")


def validate_fixture(data, source="fixture"):
    """One recorded fixture pair, checked against the recorded shape.

    Returns the pair unchanged when it is well formed and raises
    JudgmentError naming the source and the defect when it is not.
    """
    if not isinstance(data, dict):
        _fail(source, "is not an object")
    extra = set(data) - {"synthetic", "judgment_point", "request", "response"}
    if extra:
        _fail(
            source,
            "carries the field "
            + repr(sorted(extra, key=repr)[0])
            + ", which the fixture envelope does not carry",
        )
    if data.get("synthetic") is not True:
        _fail(source, "is not marked synthetic")
    point = data.get("judgment_point")
    if point not in _FIELDS_FOR_KIND:
        _fail(
            source,
            "field 'judgment_point' names no judgment point this "
            "tool records",
        )
    request = data.get("request")
    if not isinstance(request, dict):
        _fail(source, "field 'request' is not an object")
    if point == RANKING_POINT:
        _validate_ranking_request(source, request)
    else:
        _validate_request(source, request)
    if "response" not in data:
        _fail(source, "carries no response")
    _validate_response(source, data["response"], point)
    return data


def _request_key(request, point):
    return json.dumps({"point": point, "request": request}, sort_keys=True)


def load_fixture_file(path):
    target = Path(path)
    try:
        text = target.read_text(encoding="utf-8")
    except OSError as exc:
        raise JudgmentError("fixture file cannot be read, " + str(path)) from exc
    except UnicodeDecodeError as exc:
        raise JudgmentError("fixture file is not valid UTF-8, " + str(path)) from exc
    try:
        data = json.loads(text)
    except json.JSONDecodeError as exc:
        raise JudgmentError("fixture file is not valid JSON, " + str(path)) from exc
    return validate_fixture(data, source="fixture " + target.name)


class FixturePlayer:
    """Answers judgment requests from the recorded fixtures.

    Every JSON file in the directory holds one fixture pair, for
    either judgment point. Pairs are validated as they load, so a
    response that changes shape fails here, loudly, and never
    reaches the walk or the plan. Each point is answered only from
    its own pairs.
    """

    def __init__(self, directory):
        target = Path(directory)
        if not target.is_dir():
            raise JudgmentError(
                "fixtures directory cannot be read, " + str(directory)
            )
        self._answers = {}
        for path in sorted(target.glob("*.json")):
            pair = load_fixture_file(path)
            key = _request_key(pair["request"], pair["judgment_point"])
            if key in self._answers:
                raise JudgmentError(
                    "fixture "
                    + path.name
                    + " records the same request as an earlier fixture"
                )
            self._answers[key] = pair["response"]

    def _answer(self, point, request):
        response = self._answers.get(_request_key(request, point))
        if response is None:
            return None
        return json.loads(json.dumps(response))

    def judge(self, request):
        """The recorded classification response, or None to abstain."""
        return self._answer(JUDGMENT_POINT, request)

    def judge_ranking(self, request):
        """The recorded ranking response, or None to abstain."""
        return self._answer(RANKING_POINT, request)


def _check_answer_at(response, point):
    if response is None:
        return None
    try:
        _validate_response("answer", response, point)
    except JudgmentError:
        return None
    return response


def check_answer(response):
    """The answer itself when it is well formed for this point, else None.

    A judgment source that is not the fixture player can hand back
    anything. An answer outside the recorded shape for the
    classification point is treated as no judgment at all, never
    trusted and never recorded, so a malformed answer cannot crash
    the walk or reach a recall result.
    """
    return _check_answer_at(response, JUDGMENT_POINT)


def check_ranking_answer(response):
    """The answer itself when it is well formed for ranking, else None.

    The ranking twin of check_answer. An answer outside the recorded
    shape for the compensation ranking point is treated as no
    judgment at all, so a malformed answer cannot reorder a plan.
    """
    return _check_answer_at(response, RANKING_POINT)


def trusted(response):
    """Whether a judgment answer may be used at all.

    An abstention and an answer whose confidence falls below the
    threshold in the contract both count as no judgment at all.
    """
    if not isinstance(response, dict):
        return False
    if response.get("kind") != "choice":
        return False
    confidence = response.get("confidence")
    return _is_number(confidence) and float(confidence) >= CONFIDENCE_THRESHOLD


def trusted_score(response):
    """Whether a ranking answer may order a candidate at all.

    A score answer counts only when its score is a number in the
    unit interval and its confidence reaches the same threshold the
    classification point trusts. An abstention, a malformed answer,
    and an answer below the threshold all count as no judgment.
    """
    if not isinstance(response, dict):
        return False
    if response.get("kind") != "score":
        return False
    score = response.get("score")
    if not _is_number(score) or not 0.0 <= float(score) <= 1.0:
        return False
    confidence = response.get("confidence")
    return _is_number(confidence) and float(confidence) >= CONFIDENCE_THRESHOLD


def classification_request(artifact, status):
    """The classification request the walk asks about one artifact.

    The request is built from the stamp and the walk and from
    nothing else, naming the artifact, the derivation parents the
    stamp records for it sorted by path, the digest its stamp line
    records, and the status the walk computed for it.
    """
    return {
        "artifact": artifact.path,
        "parents": sorted(artifact.parents),
        "digest": artifact.digest,
        "status": status,
    }


def ranking_request(entry, candidate):
    """The ranking request the plan asks about one candidate.

    The request is built from the stamp and the walk and from
    nothing else, naming the artifact, the candidate compensation,
    the class the walk result carries for the artifact, the status
    the walk computed for it, and the chain the walk prints for it.
    """
    return {
        "artifact": entry.path,
        "candidate": candidate,
        "class": entry.rev_class,
        "status": entry.status,
        "chain": " -> ".join(entry.chain),
    }
