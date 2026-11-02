"""BB-16 to BB-21. Proposed classifications from recorded judgments."""

import json
import pathlib

import pytest

from badbatch.cli import main
from badbatch.errors import JudgmentError, RecordError, StampError
from badbatch.judgment import (
    FixturePlayer,
    check_answer,
    classification_request,
    trusted,
    validate_fixture,
)
from badbatch.records import UNCLASSED, parse_run_record
from badbatch.stamps import StampArtifact, build_stamp, format_stamp, parse_stamp
from badbatch.walk import format_recall, recall_walk

ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "fixtures"

HEADER = """run run-0021
agent writer-3
prompt-hash c07ffee
model synth-writer-2
"""

FILES = (
    'file src/report.py created\ncontent\nprint("report")\nend\n'
    "file src/data.csv created\ncontent\na,b\nend\n"
)
ART_HTML = (
    "artifact build/report.html derived-from src/report.py "
    "class unclassed\ncontent\n<html>report</html>\nend\n"
)
ART_SUMMARY = (
    "artifact docs/summary.md derived-from build/report.html "
    "class unclassed\ncontent\nsummary\nend\n"
)
ART_BUNDLE = (
    "artifact dist/bundle.zip derived-from src/data.csv "
    "class unclassed\ncontent\nbundle\nend\n"
)
ART_CHART = (
    "artifact build/chart.png derived-from src/data.csv "
    "class unclassed\ncontent\nchart\nend\n"
)
ART_PDF = (
    "artifact exports/report.pdf derived-from docs/summary.md "
    "class unclassed\ncontent\npdf\nend\n"
)
ART_PAGE = (
    "artifact build/page.html derived-from src/report.py "
    "class irreversible\ncontent\npage\nend\n"
)
ART_ZPAGE = (
    "artifact z/page.txt derived-from src/data.csv "
    "class reversible\ncontent\nzpage\nend\n"
)

PROBABILITIES = {
    "reversible": 0.9,
    "reversible-with-compensation": 0.05,
    "irreversible": 0.05,
}


def stamp_of(body, source="stamp.txt"):
    record = parse_run_record(HEADER + body, "record.txt")
    return parse_stamp(format_stamp(build_stamp(record)), source)


def state_for(stamp, drifted=(), missing=()):
    state = {}
    for entry in stamp.artifacts:
        if entry.path in missing:
            continue
        state[entry.path] = "0" * 64 if entry.path in drifted else entry.digest
    return state


def refusal_line(path, source="stamp.txt"):
    return (
        f"{source} is malformed, the artifact line for {path} "
        "names the class 'unclassed', which is not one of the "
        "three named classes"
    )


class SpyPlayer:
    """Wraps a real player and records every request it is asked."""

    def __init__(self, inner):
        self.inner = inner
        self.requests = []

    def judge(self, request):
        self.requests.append(dict(request))
        return self.inner.judge(request)


class StubSource:
    """A foreign judgment source that hands back one fixed answer."""

    def __init__(self, answer):
        self.answer = answer

    def judge(self, request):
        return self.answer


def choice_answer(confidence, choice="reversible"):
    return {
        "kind": "choice",
        "choice": choice,
        "confidence": confidence,
        "probabilities": dict(PROBABILITIES),
    }


# The unclassed marker, BB-16.


def test_the_marker_round_trips_from_record_to_stamp():
    record = parse_run_record(HEADER + FILES + ART_HTML, "record.txt")
    stamp = build_stamp(record)
    assert stamp.artifacts[0].rev_class == UNCLASSED
    text = format_stamp(stamp)
    assert "artifact build/report.html unclassed " in text
    assert parse_stamp(text, "stamp.txt").artifacts[0].rev_class == UNCLASSED


def test_a_class_word_outside_the_three_and_the_marker_is_refused():
    body = ART_HTML.replace("class unclassed", "class sideways")
    with pytest.raises(RecordError):
        parse_run_record(HEADER + FILES + body, "record.txt")
    stamp_text = format_stamp(stamp_of(FILES + ART_HTML))
    bad = stamp_text.replace(" unclassed ", " sideways ")
    with pytest.raises(StampError) as caught:
        parse_stamp(bad, "stamp.txt")
    assert str(caught.value) == (
        "stamp.txt is malformed, the artifact line for "
        "build/report.html names the class 'sideways', which is "
        "not one of the three named classes"
    )


# The recorded fixture shape, BB-17 and BB-18.


def test_every_committed_fixture_matches_the_recorded_shape():
    paths = sorted(FIXTURES.glob("*.json"))
    assert len(paths) == 5
    for path in paths:
        pair = json.loads(path.read_text())
        assert validate_fixture(pair, source=path.name) is pair


def test_the_player_answers_an_exact_match_and_abstains_otherwise():
    player = FixturePlayer(FIXTURES)
    artifact = StampArtifact(
        "build/report.html", UNCLASSED, "2" * 64, ("src/report.py",)
    )
    request = classification_request(artifact, "current")
    assert player.judge(request) is None
    committed = json.loads(
        (FIXTURES / "classification-report-html-reversible.json").read_text()
    )
    assert player.judge(committed["request"]) == committed["response"]


def write_fixture(directory, name, pair):
    directory.mkdir(exist_ok=True)
    (directory / name).write_text(json.dumps(pair))


def good_pair():
    return {
        "judgment_point": "classification",
        "request": {
            "artifact": "build/report.html",
            "parents": ["src/report.py"],
            "digest": "2" * 64,
            "status": "current",
        },
        "response": choice_answer(0.9),
        "synthetic": True,
    }


@pytest.mark.parametrize(
    "mutate",
    [
        lambda pair: pair.update(synthetic=False),
        lambda pair: pair.update(judgment_point="severity"),
        lambda pair: pair["response"].update(note="extra"),
        lambda pair: pair["response"].update(
            kind="abstention", confidence=0.9
        ),
        lambda pair: pair["response"].pop("probabilities"),
        lambda pair: pair["response"]["probabilities"].pop("irreversible"),
        lambda pair: pair["response"]["probabilities"].update(
            reversible=0.5, **{"reversible-with-compensation": 0.0}
        ),
        lambda pair: pair["response"].update(confidence=1.5),
        lambda pair: pair["request"].pop("status"),
        lambda pair: pair["request"].update(extra="field"),
        lambda pair: pair["request"].update(digest="not-hex"),
    ],
)
def test_a_fixture_outside_the_shape_is_refused_at_load(tmp_path, mutate):
    pair = good_pair()
    mutate(pair)
    write_fixture(tmp_path, "bad.json", pair)
    with pytest.raises(JudgmentError):
        FixturePlayer(tmp_path)


def test_a_fixture_that_is_not_json_is_refused_at_load(tmp_path):
    (tmp_path / "bad.json").write_text("this is not json\n")
    with pytest.raises(JudgmentError):
        FixturePlayer(tmp_path)


def test_two_fixtures_for_one_request_are_refused_at_load(tmp_path):
    write_fixture(tmp_path, "one.json", good_pair())
    write_fixture(tmp_path, "two.json", good_pair())
    with pytest.raises(JudgmentError):
        FixturePlayer(tmp_path)


def test_a_missing_fixtures_directory_is_refused_at_load(tmp_path):
    with pytest.raises(JudgmentError):
        FixturePlayer(tmp_path / "ghost")


# Answers from any source, BB-19 and BB-20.


@pytest.mark.parametrize(
    "answer",
    [
        {"kind": "choice", "choice": "reversible", "confidence": 0.9},
        dict(choice_answer(0.9), note="extra"),
        {"kind": "score", "score": 0.9, "confidence": 0.9},
        {
            "kind": "choice",
            "choice": "reversible",
            "confidence": 0.9,
            "probabilities": {"reversible": 1.0},
        },
        "not an answer at all",
    ],
)
def test_a_malformed_foreign_answer_counts_as_no_judgment(answer):
    assert check_answer(answer) is None
    assert not trusted(check_answer(answer))


def test_an_abstention_is_well_formed_but_never_trusted():
    answer = {"kind": "abstention"}
    assert check_answer(answer) == answer
    assert not trusted(answer)


def test_the_trust_threshold_edge_is_exactly_point_six():
    assert trusted(choice_answer(0.6))
    assert not trusted(choice_answer(0.599999))


# The walk rule, BB-21.


def test_without_a_source_an_unclassed_artifact_refuses_as_round_1():
    stamp = stamp_of(FILES + ART_HTML + ART_SUMMARY)
    with pytest.raises(StampError) as caught:
        recall_walk(stamp, state_for(stamp), "src/report.py")
    assert str(caught.value) == refusal_line("build/report.html")


def test_trusted_proposals_class_the_walk_with_the_marker():
    stamp = stamp_of(FILES + ART_HTML + ART_SUMMARY + ART_BUNDLE)
    player = FixturePlayer(FIXTURES)
    state = state_for(stamp, drifted={"docs/summary.md"})
    entries = recall_walk(stamp, state, "src/report.py", player)
    assert [(e.path, e.rev_class, e.proposed) for e in entries] == [
        ("build/report.html", "reversible", True),
        ("docs/summary.md", "reversible-with-compensation", True),
    ]
    assert format_recall("src/report.py", entries) == (
        "recall src/report.py\n"
        "artifact build/report.html reversible proposed current "
        "chain src/report.py -> build/report.html\n"
        "artifact docs/summary.md reversible-with-compensation "
        "proposed drifted chain src/report.py -> build/report.html "
        "-> docs/summary.md\n"
    )
    state = state_for(stamp, missing={"dist/bundle.zip"})
    entries = recall_walk(stamp, state, "src/data.csv", player)
    assert [(e.path, e.rev_class, e.status, e.proposed) for e in entries] == [
        ("dist/bundle.zip", "irreversible", "missing", True)
    ]


def test_a_proposed_class_sorts_into_its_proposed_group():
    stamp = stamp_of(FILES + ART_BUNDLE + ART_ZPAGE)
    player = FixturePlayer(FIXTURES)
    state = state_for(stamp, missing={"dist/bundle.zip"})
    entries = recall_walk(stamp, state, "src/data.csv", player)
    assert [e.path for e in entries] == ["z/page.txt", "dist/bundle.zip"]
    assert [e.proposed for e in entries] == [False, True]


def test_an_abstention_refuses_the_walk_naming_the_artifact():
    stamp = stamp_of(FILES + ART_BUNDLE + ART_CHART)
    player = FixturePlayer(FIXTURES)
    with pytest.raises(StampError) as caught:
        recall_walk(stamp, state_for(stamp), "src/data.csv", player)
    assert str(caught.value) == refusal_line("build/chart.png")


def test_a_below_threshold_answer_refuses_the_walk():
    stamp = stamp_of(FILES + ART_HTML + ART_SUMMARY + ART_PDF)
    player = FixturePlayer(FIXTURES)
    state = state_for(stamp, drifted={"docs/summary.md"})
    with pytest.raises(StampError) as caught:
        recall_walk(stamp, state, "src/report.py", player)
    assert str(caught.value) == refusal_line("exports/report.pdf")


def test_the_walk_trusts_an_answer_at_exactly_point_six():
    stamp = stamp_of(FILES + ART_HTML)
    state = state_for(stamp)
    entries = recall_walk(stamp, state, "src/report.py", StubSource(choice_answer(0.6)))
    assert entries[0].rev_class == "reversible"
    assert entries[0].proposed
    with pytest.raises(StampError):
        recall_walk(
            stamp, state, "src/report.py", StubSource(choice_answer(0.599999))
        )


def test_a_malformed_foreign_source_counts_as_no_judgment():
    stamp = stamp_of(FILES + ART_HTML)
    state = state_for(stamp)
    bad = {"kind": "choice", "choice": "reversible", "confidence": 0.9}
    with pytest.raises(StampError) as caught:
        recall_walk(stamp, state, "src/report.py", StubSource(bad))
    assert str(caught.value) == refusal_line("build/report.html")


def test_a_classed_artifact_is_never_asked_about(tmp_path):
    stamp = stamp_of(FILES + ART_PAGE + ART_HTML)
    page = next(e for e in stamp.artifacts if e.path == "build/page.html")
    rigged = {
        "judgment_point": "classification",
        "request": classification_request(page, "current"),
        "response": choice_answer(0.95),
        "synthetic": True,
    }
    html = json.loads(
        (FIXTURES / "classification-report-html-reversible.json").read_text()
    )
    write_fixture(tmp_path, "rigged.json", rigged)
    write_fixture(tmp_path, "html.json", html)
    spy = SpyPlayer(FixturePlayer(tmp_path))
    entries = recall_walk(stamp, state_for(stamp), "src/report.py", spy)
    by_path = {e.path: e for e in entries}
    assert by_path["build/page.html"].rev_class == "irreversible"
    assert not by_path["build/page.html"].proposed
    assert [r["artifact"] for r in spy.requests] == ["build/report.html"]


def test_an_unreachable_unclassed_artifact_is_never_asked_about():
    stamp = stamp_of(FILES + ART_CHART)
    spy = SpyPlayer(FixturePlayer(FIXTURES))
    entries = recall_walk(stamp, state_for(stamp), "src/report.py", spy)
    assert entries == ()
    assert spy.requests == []
    assert recall_walk(stamp, state_for(stamp), "src/report.py") == ()


def test_a_fully_classed_walk_is_byte_identical_with_and_without_a_source():
    stamp = stamp_of(FILES + ART_PAGE)
    state = state_for(stamp)
    spy = SpyPlayer(FixturePlayer(FIXTURES))
    plain = recall_walk(stamp, state, "src/report.py")
    with_source = recall_walk(stamp, state, "src/report.py", spy)
    assert plain == with_source
    assert format_recall("src/report.py", plain) == format_recall(
        "src/report.py", with_source
    )
    assert spy.requests == []


# The recall command and its fixtures flag, BB-21.


def write(tmp_path, name, text):
    path = tmp_path / name
    path.write_text(text)
    return str(path)


def cli_stamp(tmp_path, body):
    record = parse_run_record(HEADER + body, "record.txt")
    return write(tmp_path, "stamp.txt", format_stamp(build_stamp(record)))


def cli_state(tmp_path, stamp_text, drifted=(), missing=()):
    stamp = parse_stamp(stamp_text, "stamp.txt")
    lines = []
    for entry in stamp.artifacts:
        if entry.path in missing:
            continue
        digest = "0" * 64 if entry.path in drifted else entry.digest
        lines.append(f"file {entry.path} {digest}\n")
    return write(tmp_path, "state.txt", "".join(lines))


def test_recall_with_fixtures_prints_the_proposals(tmp_path, capsys):
    stamp = cli_stamp(tmp_path, FILES + ART_HTML + ART_SUMMARY)
    state = cli_state(
        tmp_path, pathlib.Path(stamp).read_text(), drifted={"docs/summary.md"}
    )
    before = (pathlib.Path(stamp).read_bytes(), pathlib.Path(state).read_bytes())
    assert (
        main(["recall", stamp, state, "src/report.py", "--fixtures", str(FIXTURES)])
        == 0
    )
    out, err = capsys.readouterr()
    assert err == ""
    assert out == (
        "recall src/report.py\n"
        "artifact build/report.html reversible proposed current "
        "chain src/report.py -> build/report.html\n"
        "artifact docs/summary.md reversible-with-compensation "
        "proposed drifted chain src/report.py -> build/report.html "
        "-> docs/summary.md\n"
    )
    after = (pathlib.Path(stamp).read_bytes(), pathlib.Path(state).read_bytes())
    assert before == after


def test_recall_without_fixtures_refuses_with_the_round_1_line(tmp_path, capsys):
    stamp = cli_stamp(tmp_path, FILES + ART_HTML)
    state = cli_state(tmp_path, pathlib.Path(stamp).read_text())
    assert main(["recall", stamp, state, "src/report.py"]) == 2
    out, err = capsys.readouterr()
    assert out == ""
    assert err == f"error: {refusal_line('build/report.html', source=stamp)}\n"


def test_recall_with_a_missing_fixtures_directory_exits_two(tmp_path, capsys):
    stamp = cli_stamp(tmp_path, FILES + ART_PAGE)
    state = cli_state(tmp_path, pathlib.Path(stamp).read_text())
    ghost = str(tmp_path / "ghost")
    assert main(["recall", stamp, state, "src/report.py", "--fixtures", ghost]) == 2
    out, err = capsys.readouterr()
    assert out == ""
    assert err.count("\n") == 1
    assert "Traceback" not in err


def test_recall_with_a_malformed_fixture_exits_two(tmp_path, capsys):
    stamp = cli_stamp(tmp_path, FILES + ART_HTML)
    state = cli_state(tmp_path, pathlib.Path(stamp).read_text())
    bad_dir = tmp_path / "bad-fixtures"
    bad_dir.mkdir()
    (bad_dir / "bad.json").write_text("this is not json\n")
    assert (
        main(["recall", stamp, state, "src/report.py", "--fixtures", str(bad_dir)])
        == 2
    )
    out, err = capsys.readouterr()
    assert out == ""
    assert err.count("\n") == 1
    assert "Traceback" not in err


def test_a_fully_classed_recall_is_byte_identical_with_and_without_the_flag(
    tmp_path, capsys
):
    stamp = cli_stamp(tmp_path, FILES + ART_PAGE)
    state = cli_state(tmp_path, pathlib.Path(stamp).read_text())
    assert main(["recall", stamp, state, "src/report.py"]) == 0
    plain, _ = capsys.readouterr()
    assert (
        main(["recall", stamp, state, "src/report.py", "--fixtures", str(FIXTURES)])
        == 0
    )
    with_flag, _ = capsys.readouterr()
    assert plain == with_flag
    assert "proposed" not in plain
