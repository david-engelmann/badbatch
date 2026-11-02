"""BB-23. Hardening completion and the stated readings."""

import pytest

from badbatch.errors import JudgmentError, StampError
from badbatch.judgment import FixturePlayer, check_answer, trusted, validate_fixture
from badbatch.records import parse_run_record
from badbatch.stamps import build_stamp, format_stamp, parse_stamp
from badbatch.walk import format_recall, recall_walk

PROBABILITIES = {
    "reversible": 0.9,
    "reversible-with-compensation": 0.05,
    "irreversible": 0.05,
}

HEADER = """run run-0021
agent writer-3
prompt-hash c07ffee
model synth-writer-2
"""

RECORD_BODY = (
    'file src/report.py created\ncontent\nprint("report")\nend\n'
    "artifact build/report.html derived-from src/report.py "
    "class unclassed\ncontent\n<html>report</html>\nend\n"
)

STAMP_HEADER = (
    "stamp run-0009\n"
    "agent writer-3\n"
    "prompt-hash b41d90\n"
    "model synth-writer-2\n"
)

REQUEST = {
    "artifact": "build/report.html",
    "parents": ["src/report.py"],
    "digest": "0" * 64,
    "status": "current",
}


def mixed_key_answer():
    answer = {
        "kind": "choice",
        "choice": "reversible",
        "confidence": 0.9,
        "probabilities": dict(PROBABILITIES),
    }
    answer[7] = "an integer key"
    answer["note"] = "a string key"
    return answer


class StubSource:
    """A foreign judgment source that hands back one fixed answer."""

    def __init__(self, answer):
        self.answer = answer

    def judge(self, request):
        return self.answer


# The response ordering rule, BB-23 first rule.


def test_an_answer_with_mixed_type_extra_keys_counts_as_no_judgment():
    assert check_answer(mixed_key_answer()) is None
    assert not trusted(check_answer(mixed_key_answer()))


def test_a_walk_judged_by_a_mixed_key_answer_refuses_as_with_no_source():
    record = parse_run_record(HEADER + RECORD_BODY, "record.txt")
    stamp = parse_stamp(format_stamp(build_stamp(record)), "stamp.txt")
    state = {entry.path: entry.digest for entry in stamp.artifacts}
    expected = (
        "stamp.txt is malformed, the artifact line for "
        "build/report.html names the class 'unclassed', which is "
        "not one of the three named classes"
    )
    with pytest.raises(StampError) as no_source:
        recall_walk(stamp, state, "src/report.py")
    assert str(no_source.value) == expected
    with pytest.raises(StampError) as mixed_source:
        recall_walk(
            stamp, state, "src/report.py", player=StubSource(mixed_key_answer())
        )
    assert str(mixed_source.value) == expected


def test_an_envelope_with_mixed_type_extra_keys_names_by_printed_form():
    pair = {
        "synthetic": True,
        "judgment_point": "classification",
        "request": dict(REQUEST),
        "response": {"kind": "abstention"},
    }
    pair[7] = "an integer key"
    pair["note"] = "a string key"
    with pytest.raises(JudgmentError) as caught:
        validate_fixture(pair)
    assert str(caught.value) == (
        "fixture carries the field 'note', which the fixture "
        "envelope does not carry"
    )


# The stamp readings, BB-23 second rule.


def test_a_cyclic_stamp_is_admitted_and_the_walk_terminates_in_order():
    stamp = parse_stamp(
        STAMP_HEADER
        + "file src/seed.txt created " + "c" * 64 + "\n"
        + "artifact out/a.html reversible " + "a" * 64
        + " derived-from src/seed.txt, out/b.html\n"
        + "artifact out/b.html irreversible " + "b" * 64
        + " derived-from out/a.html\n",
        "stamp.txt",
    )
    state = {"out/a.html": "a" * 64, "out/b.html": "b" * 64}
    entries = recall_walk(stamp, state, "src/seed.txt")
    assert format_recall("src/seed.txt", entries) == (
        "recall src/seed.txt\n"
        "artifact out/a.html reversible current "
        "chain src/seed.txt -> out/a.html\n"
        "artifact out/b.html irreversible current "
        "chain src/seed.txt -> out/a.html -> out/b.html\n"
    )


def test_a_deleted_file_line_carrying_a_digest_is_admitted_and_kept():
    digest = "d" * 64
    with_digest = parse_stamp(
        STAMP_HEADER
        + "file src/old.txt deleted " + digest + "\n"
        + "artifact out/a.html reversible " + "a" * 64
        + " derived-from src/old.txt\n",
        "stamp.txt",
    )
    with_none = parse_stamp(
        STAMP_HEADER
        + "file src/old.txt deleted none\n"
        + "artifact out/a.html reversible " + "a" * 64
        + " derived-from src/old.txt\n",
        "stamp.txt",
    )
    assert with_digest.files[0].digest == digest
    assert with_none.files[0].digest is None
    assert (
        "file src/old.txt deleted " + digest
        in format_stamp(with_digest).splitlines()
    )
    state = {"out/a.html": "a" * 64}
    assert recall_walk(with_digest, state, "src/old.txt") == recall_walk(
        with_none, state, "src/old.txt"
    )


# The duplicate key reading, BB-23 third rule.


def fixture_text(first_kind, last_kind):
    return (
        '{"synthetic": true, "judgment_point": "classification", '
        '"request": {"artifact": "build/report.html", '
        '"parents": ["src/report.py"], "digest": "' + "0" * 64 + '", '
        '"status": "current"}, '
        '"response": {"kind": "' + first_kind + '", "kind": "'
        + last_kind + '", "choice": "reversible", "confidence": 0.9, '
        '"probabilities": {"reversible": 0.9, '
        '"reversible-with-compensation": 0.05, "irreversible": 0.05}}}'
    )


def test_a_fixture_with_a_duplicated_kind_field_is_judged_on_the_last_kind(
    tmp_path,
):
    (tmp_path / "duplicated.json").write_text(
        fixture_text("score", "choice"), encoding="utf-8"
    )
    player = FixturePlayer(tmp_path)
    answer = player.judge(dict(REQUEST))
    assert answer is not None
    assert answer["kind"] == "choice"
    assert trusted(answer)


def test_a_fixture_whose_last_duplicated_kind_is_invalid_is_refused(tmp_path):
    (tmp_path / "duplicated.json").write_text(
        fixture_text("choice", "score"), encoding="utf-8"
    )
    with pytest.raises(JudgmentError):
        FixturePlayer(tmp_path)
