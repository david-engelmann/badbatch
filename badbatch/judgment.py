"""Recorded decision model judgments, played back from fixtures.

The roadmap gives badbatch a proposed classification point. Where
the stamp data bounds no class for an artifact, a judgment may
propose one of the three reversibility classes for that artifact.
The proposal is advisory. It is marked as proposed wherever the
walk output shows it, it never becomes the recorded class, and no
judgment executes, approves, or triggers a recall.

A judgment is recorded as a fixture that pairs one classification
request with one response in the shape a decision model returns. A
choice response carries the chosen label, a confidence, and a
probability for every label. An abstention response carries none of
those fields. The fixture player answers a request only from a
recorded fixture whose request matches it exactly, and abstains
where no fixture matches. Everything here is offline. The player
reads files from one directory and nothing else.
"""

import json
import math
import re
from pathlib import Path

from .errors import JudgmentError
from .records import CLASSES

CONFIDENCE_THRESHOLD = 0.6

JUDGMENT_POINT = "classification"

_DIGEST = re.compile(r"[0-9a-f]{64}")
_REQUEST_FIELDS = {"artifact", "parents", "digest", "status"}
_STATUSES = ("current", "drifted", "missing")
_FIELDS_FOR_KIND = {
    "abstention": {"kind"},
    "choice": {"kind", "choice", "confidence", "probabilities"},
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


def _validate_response(source, response):
    if not isinstance(response, dict):
        _fail(source, "response is not an object")
    kind = response.get("kind")
    if isinstance(kind, str) and kind in _FIELDS_FOR_KIND:
        extra = set(response) - _FIELDS_FOR_KIND[kind]
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
    if data.get("judgment_point") != JUDGMENT_POINT:
        _fail(source, "field 'judgment_point' is not the classification point")
    request = data.get("request")
    if not isinstance(request, dict):
        _fail(source, "field 'request' is not an object")
    _validate_request(source, request)
    if "response" not in data:
        _fail(source, "carries no response")
    _validate_response(source, data["response"])
    return data


def _request_key(request):
    return json.dumps(
        {"point": JUDGMENT_POINT, "request": request}, sort_keys=True
    )


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
    """Answers classification requests from the recorded fixtures.

    Every JSON file in the directory holds one fixture pair. Pairs
    are validated as they load, so a response that changes shape
    fails here, loudly, and never reaches the walk.
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
            key = _request_key(pair["request"])
            if key in self._answers:
                raise JudgmentError(
                    "fixture "
                    + path.name
                    + " records the same request as an earlier fixture"
                )
            self._answers[key] = pair["response"]

    def judge(self, request):
        """The recorded response for this request, or None to abstain."""
        response = self._answers.get(_request_key(request))
        if response is None:
            return None
        return json.loads(json.dumps(response))


def check_answer(response):
    """The answer itself when it is well formed for this point, else None.

    A judgment source that is not the fixture player can hand back
    anything. An answer outside the recorded shape for the
    classification point is treated as no judgment at all, never
    trusted and never recorded, so a malformed answer cannot crash
    the walk or reach a recall result.
    """
    if response is None:
        return None
    try:
        _validate_response("answer", response)
    except JudgmentError:
        return None
    return response


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
