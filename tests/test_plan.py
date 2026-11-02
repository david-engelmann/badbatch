"""BB-24 to BB-29. The execution plan and the ranking point."""

import json
import pathlib

import pytest

from badbatch.cli import main
from badbatch.errors import JudgmentError
from badbatch.judgment import (
    FixturePlayer,
    check_ranking_answer,
    classification_request,
    ranking_request,
    trusted_score,
    validate_fixture,
)
from badbatch.plan import build_plan, candidates_for, format_plan
from badbatch.records import parse_run_record
from badbatch.stamps import build_stamp, format_stamp, parse_stamp
from badbatch.walk import recall_walk

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "fixtures"
RANKING_FIXTURES = ROOT / "fixtures-ranking"

REBUILD = "rebuild-from-parents"
REMOVE = "remove-artifact"
QUARANTINE = "quarantine-artifact"

SESSION = """run run-0031
agent writer-3
prompt-hash d15ea5e
model synth-writer-2

file src/base.py created
content
print("base")
end

artifact out/alpha.html derived-from src/base.py class reversible-with-compensation
content
alpha
end

artifact out/beta.html derived-from src/base.py class reversible
content
beta
end

artifact out/gamma.html derived-from src/base.py class reversible
content
gamma
end

artifact out/delta.html derived-from src/base.py class reversible-with-compensation
content
delta
end
"""

DRIFTED = {"out/beta.html", "out/delta.html"}

PLAIN_PLAN = (
    "plan src/base.py\n"
    "artifact out/beta.html reversible drifted "
    "chain src/base.py -> out/beta.html\n"
    "compensation quarantine-artifact\n"
    "compensation remove-artifact\n"
    "artifact out/gamma.html reversible current "
    "chain src/base.py -> out/gamma.html\n"
    "compensation rebuild-from-parents\n"
    "compensation remove-artifact\n"
    "artifact out/alpha.html reversible-with-compensation current "
    "chain src/base.py -> out/alpha.html\n"
    "compensation rebuild-from-parents\n"
    "compensation remove-artifact\n"
    "compensation quarantine-artifact\n"
    "artifact out/delta.html reversible-with-compensation drifted "
    "chain src/base.py -> out/delta.html\n"
    "compensation quarantine-artifact\n"
    "compensation remove-artifact\n"
)

RANKED_PLAN = (
    "plan src/base.py\n"
    "artifact out/beta.html reversible drifted "
    "chain src/base.py -> out/beta.html\n"
    "compensation quarantine-artifact score 0.8\n"
    "compensation remove-artifact score 0.8\n"
    "artifact out/gamma.html reversible current "
    "chain src/base.py -> out/gamma.html\n"
    "compensation rebuild-from-parents\n"
    "compensation remove-artifact\n"
    "artifact out/alpha.html reversible-with-compensation current "
    "chain src/base.py -> out/alpha.html\n"
    "compensation remove-artifact score 0.95\n"
    "compensation quarantine-artifact score 0.7\n"
    "compensation rebuild-from-parents score 0.62\n"
    "artifact out/delta.html reversible-with-compensation drifted "
    "chain src/base.py -> out/delta.html\n"
    "compensation quarantine-artifact\n"
    "compensation remove-artifact\n"
)


def session_stamp():
    record = parse_run_record(SESSION, "record.txt")
    return parse_stamp(format_stamp(build_stamp(record)), "stamp.txt")


def session_state(stamp):
    state = {}
    for entry in stamp.artifacts:
        state[entry.path] = "0" * 64 if entry.path in DRIFTED else entry.digest
    return state


class CapturingSource:
    """Records every request at both points.

    Answers nothing by default. A classification answer may be
    fixed for sessions whose walk needs a proposal to proceed.
    """

    def __init__(self, classification_answer=None):
        self.classification_requests = []
        self.ranking_requests = []
        self.classification_answer = classification_answer

    def judge(self, request):
        self.classification_requests.append(dict(request))
        return self.classification_answer

    def judge_ranking(self, request):
        self.ranking_requests.append(dict(request))
        return None


class StubRankingSource:
    """Answers ranking requests from a fixed table, nothing else."""

    def __init__(self, answers):
        self.answers = answers
        self.ranking_requests = []

    def judge(self, request):
        return None

    def judge_ranking(self, request):
        self.ranking_requests.append(dict(request))
        return self.answers.get((request["artifact"], request["candidate"]))


def score_answer(score_value, confidence):
    return {"kind": "score", "score": score_value, "confidence": confidence}


# The candidate table, BB-25.


@pytest.mark.parametrize(
    "rev_class,status,expected",
    [
        ("reversible", "current", (REBUILD, REMOVE)),
        (
            "reversible-with-compensation",
            "current",
            (REBUILD, REMOVE, QUARANTINE),
        ),
        ("irreversible", "current", (QUARANTINE,)),
        ("reversible", "drifted", (QUARANTINE, REMOVE)),
        ("reversible-with-compensation", "drifted", (QUARANTINE, REMOVE)),
        ("irreversible", "drifted", (QUARANTINE, REMOVE)),
        ("reversible", "missing", ()),
        ("reversible-with-compensation", "missing", ()),
        ("irreversible", "missing", ()),
    ],
)
def test_the_candidate_table_by_class_and_status(rev_class, status, expected):
    assert candidates_for(rev_class, status) == expected


# The deterministic plan document, BB-24 and BB-25.


def test_the_plain_plan_over_the_session_is_exact():
    stamp = session_stamp()
    planned = build_plan(stamp, session_state(stamp), "src/base.py")
    assert format_plan("src/base.py", planned) == PLAIN_PLAN


def test_a_missing_artifact_prints_the_none_required_line():
    stamp = session_stamp()
    state = session_state(stamp)
    del state["out/gamma.html"]
    planned = build_plan(stamp, state, "src/base.py")
    text = format_plan("src/base.py", planned)
    assert (
        "artifact out/gamma.html reversible missing "
        "chain src/base.py -> out/gamma.html\n"
        "compensation none required, the artifact is already absent\n"
    ) in text


def test_a_plan_with_no_downstream_artifacts_is_the_header_alone():
    record = parse_run_record(
        "run run-0032\nagent writer-3\nprompt-hash d15ea5e\n"
        "model synth-writer-2\n\nfile src/lonely.py created\n"
        "content\nprint(1)\nend\n",
        "record.txt",
    )
    stamp = parse_stamp(format_stamp(build_stamp(record)), "stamp.txt")
    planned = build_plan(stamp, {}, "src/lonely.py")
    assert planned == ()
    assert format_plan("src/lonely.py", planned) == "plan src/lonely.py\n"


# The ranking point over the committed fixtures, BB-26 to BB-28.


def test_the_committed_ranking_fixtures_rank_the_session():
    stamp = session_stamp()
    player = FixturePlayer(RANKING_FIXTURES)
    planned = build_plan(stamp, session_state(stamp), "src/base.py", player)
    assert format_plan("src/base.py", planned) == RANKED_PLAN


def test_every_committed_ranking_fixture_matches_the_recorded_shape():
    paths = sorted(RANKING_FIXTURES.glob("*.json"))
    assert len(paths) == 9
    for path in paths:
        pair = json.loads(path.read_text())
        assert validate_fixture(pair, source=path.name) is pair


def test_the_committed_ranking_requests_are_the_builders_requests():
    stamp = session_stamp()
    source = CapturingSource()
    build_plan(stamp, session_state(stamp), "src/base.py", source)
    built = {
        json.dumps(request, sort_keys=True)
        for request in source.ranking_requests
    }
    recorded = set()
    for path in sorted(RANKING_FIXTURES.glob("*.json")):
        pair = json.loads(path.read_text())
        recorded.add(json.dumps(pair["request"], sort_keys=True))
    assert recorded == built
    assert source.classification_requests == []


def test_the_ranking_request_shape_is_exactly_five_fields():
    stamp = session_stamp()
    entries = recall_walk(stamp, session_state(stamp), "src/base.py")
    entry = next(e for e in entries if e.path == "out/alpha.html")
    assert ranking_request(entry, REMOVE) == {
        "artifact": "out/alpha.html",
        "candidate": "remove-artifact",
        "class": "reversible-with-compensation",
        "status": "current",
        "chain": "src/base.py -> out/alpha.html",
    }


def test_no_ranking_request_for_artifacts_with_fewer_than_two_candidates():
    examples = ROOT / "examples"
    stamp = parse_stamp(
        (examples / "stamp.txt").read_text(), "examples/stamp.txt"
    )
    state = {}
    for line in (examples / "current-state.txt").read_text().splitlines():
        if line.strip():
            _, path, digest = line.split()
            state[path] = digest
    source = CapturingSource()
    planned = build_plan(stamp, state, "src/report.py", source)
    asked = {(r["artifact"], r["candidate"]) for r in source.ranking_requests}
    # build/report.html carries two candidates and is asked about
    # both. docs/summary.md is missing and exports/report.pdf carries
    # one candidate, and neither is ever asked about.
    assert asked == {
        ("build/report.html", REBUILD),
        ("build/report.html", REMOVE),
    }
    text = format_plan("src/report.py", planned)
    assert "compensation none required, the artifact is already absent" in text
    assert "artifact exports/report.pdf irreversible current" in text


def test_a_plan_whose_source_abstains_everywhere_is_byte_identical():
    stamp = session_stamp()
    state = session_state(stamp)
    plain = format_plan("src/base.py", build_plan(stamp, state, "src/base.py"))
    abstaining = StubRankingSource({})
    with_source = format_plan(
        "src/base.py", build_plan(stamp, state, "src/base.py", abstaining)
    )
    assert with_source == plain == PLAIN_PLAN
    assert len(abstaining.ranking_requests) == 9


# The ordering rule, BB-28.


def test_a_tie_keeps_canonical_order_and_prints_both_scores():
    stamp = session_stamp()
    source = StubRankingSource(
        {
            ("out/gamma.html", REBUILD): score_answer(0.5, 0.9),
            ("out/gamma.html", REMOVE): score_answer(0.5, 0.9),
        }
    )
    text = format_plan(
        "src/base.py", build_plan(stamp, session_state(stamp), "src/base.py", source)
    )
    assert (
        "compensation rebuild-from-parents score 0.5\n"
        "compensation remove-artifact score 0.5\n"
    ) in text


def test_a_scored_candidate_prints_before_an_unscored_one():
    stamp = session_stamp()
    source = StubRankingSource(
        {("out/gamma.html", REMOVE): score_answer(0.7, 0.9)}
    )
    text = format_plan(
        "src/base.py", build_plan(stamp, session_state(stamp), "src/base.py", source)
    )
    assert (
        "compensation remove-artifact score 0.7\n"
        "compensation rebuild-from-parents\n"
    ) in text


def test_the_ranking_trust_edge_is_exactly_point_six():
    stamp = session_stamp()
    state = session_state(stamp)
    at = StubRankingSource(
        {("out/gamma.html", REMOVE): score_answer(0.7, 0.6)}
    )
    text = format_plan("src/base.py", build_plan(stamp, state, "src/base.py", at))
    assert "compensation remove-artifact score 0.7\n" in text
    below = StubRankingSource(
        {("out/gamma.html", REMOVE): score_answer(0.7, 0.599999)}
    )
    text = format_plan(
        "src/base.py", build_plan(stamp, state, "src/base.py", below)
    )
    assert "score" not in text


def test_trusted_score_and_check_ranking_answer_units():
    good = score_answer(0.7, 0.9)
    assert check_ranking_answer(good) == good
    assert trusted_score(good)
    assert check_ranking_answer({"kind": "abstention"}) == {"kind": "abstention"}
    assert not trusted_score({"kind": "abstention"})
    assert not trusted_score(score_answer(0.7, 0.45))
    assert not trusted_score({"kind": "score", "score": 0.7})


@pytest.mark.parametrize(
    "answer",
    [
        {
            "kind": "choice",
            "choice": "reversible",
            "confidence": 0.9,
            "probabilities": {
                "reversible": 0.9,
                "reversible-with-compensation": 0.05,
                "irreversible": 0.05,
            },
        },
        {"kind": "score", "score": 0.9, "confidence": 0.9, "note": "extra"},
        {"kind": "score", "score": 10**400, "confidence": 0.9},
        {"kind": "score", "confidence": 0.9},
        {"kind": "abstention", "score": 0.9},
        "not an answer at all",
    ],
)
def test_a_malformed_ranking_answer_counts_as_no_judgment(answer):
    assert check_ranking_answer(answer) is None
    stamp = session_stamp()
    source = StubRankingSource(
        {
            (artifact, candidate): answer
            for artifact in (
                "out/alpha.html",
                "out/beta.html",
                "out/gamma.html",
                "out/delta.html",
            )
            for candidate in (REBUILD, REMOVE, QUARANTINE)
        }
    )
    text = format_plan(
        "src/base.py",
        build_plan(stamp, session_state(stamp), "src/base.py", source),
    )
    assert text == PLAIN_PLAN


def test_a_ranking_answer_with_mixed_type_extra_keys_is_no_judgment():
    answer = score_answer(0.9, 0.9)
    answer[7] = "an integer key"
    answer["note"] = "a string key"
    assert check_ranking_answer(answer) is None


# The ranking fixture shape, BB-27.


def good_ranking_pair():
    return {
        "judgment_point": "compensation-ranking",
        "request": {
            "artifact": "out/alpha.html",
            "candidate": "remove-artifact",
            "class": "reversible-with-compensation",
            "status": "current",
            "chain": "src/base.py -> out/alpha.html",
        },
        "response": score_answer(0.95, 0.9),
        "synthetic": True,
    }


def write_fixture(directory, name, pair):
    directory.mkdir(exist_ok=True)
    (directory / name).write_text(json.dumps(pair))


@pytest.mark.parametrize(
    "mutate",
    [
        lambda pair: pair.update(synthetic=False),
        lambda pair: pair.update(judgment_point="classification"),
        lambda pair: pair.update(comment="extra envelope field"),
        lambda pair: pair["response"].pop("confidence"),
        lambda pair: pair["response"].update(score=1.5),
        lambda pair: pair["response"].update(kind="choice"),
        lambda pair: pair["request"].pop("chain"),
        lambda pair: pair["request"].update(extra="field"),
        lambda pair: pair["request"].update(candidate="delete-artifact"),
        lambda pair: pair["request"].update(**{"class": "sideways"}),
    ],
)
def test_a_ranking_fixture_outside_the_shape_is_refused_at_load(
    tmp_path, mutate
):
    pair = good_ranking_pair()
    mutate(pair)
    write_fixture(tmp_path, "bad.json", pair)
    with pytest.raises(JudgmentError):
        FixturePlayer(tmp_path)


def test_two_ranking_fixtures_for_one_request_are_refused_at_load(tmp_path):
    write_fixture(tmp_path, "one.json", good_ranking_pair())
    write_fixture(tmp_path, "two.json", good_ranking_pair())
    with pytest.raises(JudgmentError):
        FixturePlayer(tmp_path)


def test_one_directory_answers_each_point_only_from_its_own_pairs(tmp_path):
    for path in sorted(FIXTURES.glob("*.json")):
        write_fixture(tmp_path, path.name, json.loads(path.read_text()))
    for path in sorted(RANKING_FIXTURES.glob("*.json")):
        write_fixture(tmp_path, path.name, json.loads(path.read_text()))
    player = FixturePlayer(tmp_path)
    classification = json.loads(
        (FIXTURES / "classification-report-html-reversible.json").read_text()
    )
    assert player.judge(classification["request"]) == classification["response"]
    assert player.judge_ranking(classification["request"]) is None
    ranking = json.loads(
        (RANKING_FIXTURES / "ranking-alpha-remove-artifact.json").read_text()
    )
    assert player.judge_ranking(ranking["request"]) == ranking["response"]
    assert player.judge(ranking["request"]) is None


# Both points inside one plan, BB-29 and the proposed class rule.


def test_a_proposed_class_plans_under_its_proposed_class(tmp_path):
    body = (
        'file src/base.py created\ncontent\nprint("base")\nend\n'
        "artifact out/page.html derived-from src/base.py "
        "class unclassed\ncontent\npage\nend\n"
        "artifact out/tool.html derived-from src/base.py "
        "class reversible\ncontent\ntool\nend\n"
    )
    record = parse_run_record(
        "run run-0033\nagent writer-3\nprompt-hash d15ea5e\n"
        "model synth-writer-2\n\n" + body,
        "record.txt",
    )
    stamp = parse_stamp(format_stamp(build_stamp(record)), "stamp.txt")
    state = {entry.path: entry.digest for entry in stamp.artifacts}
    probabilities = {
        "reversible": 0.9,
        "reversible-with-compensation": 0.05,
        "irreversible": 0.05,
    }
    choice_answer = {
        "kind": "choice",
        "choice": "reversible",
        "confidence": 0.9,
        "probabilities": probabilities,
    }
    capture = CapturingSource(classification_answer=choice_answer)
    build_plan(stamp, state, "src/base.py", capture)
    assert len(capture.classification_requests) == 1
    classification_pair = {
        "synthetic": True,
        "judgment_point": "classification",
        "request": capture.classification_requests[0],
        "response": choice_answer,
    }
    write_fixture(tmp_path, "classification.json", classification_pair)
    for request in capture.ranking_requests:
        response = {"kind": "abstention"}
        if request["artifact"] == "out/page.html":
            response = score_answer(
                0.9 if request["candidate"] == REMOVE else 0.7, 0.9
            )
        pair = {
            "synthetic": True,
            "judgment_point": "compensation-ranking",
            "request": request,
            "response": response,
        }
        name = (
            "ranking-"
            + request["artifact"].split("/")[-1].split(".")[0]
            + "-"
            + request["candidate"]
            + ".json"
        )
        write_fixture(tmp_path, name, pair)
    planned = build_plan(stamp, state, "src/base.py", FixturePlayer(tmp_path))
    assert format_plan("src/base.py", planned) == (
        "plan src/base.py\n"
        "artifact out/page.html reversible proposed current "
        "chain src/base.py -> out/page.html\n"
        "compensation remove-artifact score 0.9\n"
        "compensation rebuild-from-parents score 0.7\n"
        "artifact out/tool.html reversible current "
        "chain src/base.py -> out/tool.html\n"
        "compensation rebuild-from-parents\n"
        "compensation remove-artifact\n"
    )


def test_the_recall_is_unchanged_by_ranking_pairs_in_its_directory(
    tmp_path, capsys
):
    mixed = tmp_path / "mixed"
    for path in sorted(FIXTURES.glob("*.json")):
        write_fixture(mixed, path.name, json.loads(path.read_text()))
    for path in sorted(RANKING_FIXTURES.glob("*.json")):
        write_fixture(mixed, path.name, json.loads(path.read_text()))
    body = (
        'file src/report.py created\ncontent\nprint("report")\nend\n'
        "file src/data.csv created\ncontent\na,b\nend\n"
        "artifact build/report.html derived-from src/report.py "
        "class unclassed\ncontent\n<html>report</html>\nend\n"
        "artifact docs/summary.md derived-from build/report.html "
        "class unclassed\ncontent\nsummary\nend\n"
    )
    record = parse_run_record(
        "run run-0021\nagent writer-3\nprompt-hash c07ffee\n"
        "model synth-writer-2\n\n" + body,
        "record.txt",
    )
    stamp_path = tmp_path / "stamp.txt"
    stamp_path.write_text(format_stamp(build_stamp(record)))
    stamp = parse_stamp(stamp_path.read_text(), "stamp.txt")
    lines = []
    for entry in stamp.artifacts:
        digest = "0" * 64 if entry.path == "docs/summary.md" else entry.digest
        lines.append(f"file {entry.path} {digest}\n")
    state_path = tmp_path / "state.txt"
    state_path.write_text("".join(lines))
    args = ["recall", str(stamp_path), str(state_path), "src/report.py"]
    assert main(args + ["--fixtures", str(FIXTURES)]) == 0
    plain, _ = capsys.readouterr()
    assert main(args + ["--fixtures", str(mixed)]) == 0
    with_ranking, _ = capsys.readouterr()
    assert with_ranking == plain
    assert "proposed" in with_ranking


# The plan command, BB-24 and BB-29.


def write(tmp_path, name, text):
    path = tmp_path / name
    path.write_text(text, encoding="utf-8")
    return str(path)


def test_the_plan_command_prints_the_plan_and_exits_zero(tmp_path, capsys):
    examples = ROOT / "examples"
    stamp = write(tmp_path, "stamp.txt", (examples / "stamp.txt").read_text())
    state = write(
        tmp_path, "state.txt", (examples / "current-state.txt").read_text()
    )
    before = (pathlib.Path(stamp).read_bytes(), pathlib.Path(state).read_bytes())
    assert main(["plan", stamp, state, "src/report.py"]) == 0
    out, err = capsys.readouterr()
    assert err == ""
    assert out == (
        "plan src/report.py\n"
        "artifact build/report.html reversible current "
        "chain src/report.py -> build/report.html\n"
        "compensation rebuild-from-parents\n"
        "compensation remove-artifact\n"
        "artifact docs/summary.md reversible-with-compensation missing "
        "chain src/report.py -> build/report.html -> docs/summary.md\n"
        "compensation none required, the artifact is already absent\n"
        "artifact exports/report.pdf irreversible current "
        "chain src/report.py -> build/report.html -> docs/summary.md "
        "-> exports/report.pdf\n"
        "compensation quarantine-artifact\n"
    )
    after = (pathlib.Path(stamp).read_bytes(), pathlib.Path(state).read_bytes())
    assert before == after


def test_the_plan_command_ranks_with_the_committed_fixtures(
    tmp_path, capsys
):
    record = parse_run_record(SESSION, "record.txt")
    stamp = write(tmp_path, "stamp.txt", format_stamp(build_stamp(record)))
    parsed = parse_stamp(pathlib.Path(stamp).read_text(), "stamp.txt")
    lines = []
    for entry in parsed.artifacts:
        digest = "0" * 64 if entry.path in DRIFTED else entry.digest
        lines.append(f"file {entry.path} {digest}\n")
    state = write(tmp_path, "state.txt", "".join(lines))
    assert (
        main(["plan", stamp, state, "src/base.py", "--fixtures",
              str(RANKING_FIXTURES)])
        == 0
    )
    out, err = capsys.readouterr()
    assert err == ""
    assert out == RANKED_PLAN


def test_the_plan_command_refuses_an_unproposed_unclassed_artifact(
    tmp_path, capsys
):
    body = (
        'file src/base.py created\ncontent\nprint("base")\nend\n'
        "artifact out/page.html derived-from src/base.py "
        "class unclassed\ncontent\npage\nend\n"
    )
    record = parse_run_record(
        "run run-0033\nagent writer-3\nprompt-hash d15ea5e\n"
        "model synth-writer-2\n\n" + body,
        "record.txt",
    )
    stamp = write(tmp_path, "stamp.txt", format_stamp(build_stamp(record)))
    parsed = parse_stamp(pathlib.Path(stamp).read_text(), "stamp.txt")
    state = write(
        tmp_path,
        "state.txt",
        "".join(
            f"file {entry.path} {entry.digest}\n" for entry in parsed.artifacts
        ),
    )
    assert main(["plan", stamp, state, "src/base.py"]) == 2
    out, err = capsys.readouterr()
    assert out == ""
    assert err == (
        f"error: {stamp} is malformed, the artifact line for "
        "out/page.html names the class 'unclassed', which is not "
        "one of the three named classes\n"
    )


def test_the_plan_command_with_a_missing_fixtures_directory_exits_two(
    tmp_path, capsys
):
    examples = ROOT / "examples"
    stamp = write(tmp_path, "stamp.txt", (examples / "stamp.txt").read_text())
    state = write(
        tmp_path, "state.txt", (examples / "current-state.txt").read_text()
    )
    ghost = str(tmp_path / "ghost")
    assert (
        main(["plan", stamp, state, "src/report.py", "--fixtures", ghost]) == 2
    )
    out, err = capsys.readouterr()
    assert out == ""
    assert err.count("\n") == 1
    assert "Traceback" not in err
