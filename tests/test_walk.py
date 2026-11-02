"""BB-6 to BB-10. Current states and the recall walk."""

import pytest

from badbatch.errors import StateError, WalkError
from badbatch.records import parse_run_record
from badbatch.stamps import build_stamp, parse_stamp
from badbatch.walk import format_recall, parse_current_state, recall_walk

HEADER = """run run-0007
agent writer-3
prompt-hash b41d90
model synth-writer-2
"""

# A diamond. src/core.py feeds gen/left.txt and gen/right.txt, and
# out/final.txt derives from both, so the walk owes three artifacts.
DIAMOND = (
    "file src/core.py created\ncontent\ncore\nend\n"
    "file src/other.py created\ncontent\nother\nend\n"
    "artifact gen/left.txt derived-from src/core.py class reversible\ncontent\nleft\nend\n"
    "artifact gen/right.txt derived-from src/core.py class reversible\ncontent\nright\nend\n"
    "artifact out/final.txt derived-from gen/left.txt, gen/right.txt class irreversible\ncontent\nfinal\nend\n"
    "artifact out/side.txt derived-from src/other.py class reversible\ncontent\nside\nend\n"
)


def stamp_of(body):
    record = parse_run_record(HEADER + body, "record.txt")
    return build_stamp(record)


def full_state(stamp):
    return {entry.path: entry.digest for entry in stamp.artifacts}


def test_state_lines_parse_and_unknown_paths_are_allowed():
    stamp = stamp_of(DIAMOND)
    text = "".join(
        f"file {entry.path} {entry.digest}\n" for entry in stamp.artifacts
    )
    text += "file README.md " + "0" * 64 + "\n"
    state = parse_current_state(text, "state.txt")
    assert state["gen/left.txt"] == stamp.artifacts[0].digest
    assert "README.md" in state


@pytest.mark.parametrize(
    "text",
    [
        "file gen/left.txt not-a-digest\n",
        "file gen/left.txt " + "0" * 64 + "\nfile gen/left.txt " + "0" * 64 + "\n",
        "frobnicate gen/left.txt " + "0" * 64 + "\n",
        "file gen/left.txt none\n",
    ],
)
def test_malformed_states_are_refused(text):
    with pytest.raises(StateError):
        parse_current_state(text, "state.txt")


def test_the_walk_lists_every_reachable_artifact_exactly_once():
    stamp = stamp_of(DIAMOND)
    entries = recall_walk(stamp, full_state(stamp), "src/core.py")
    assert [entry.path for entry in entries] == [
        "gen/left.txt",
        "gen/right.txt",
        "out/final.txt",
    ]
    # the artifact reachable only from the other file is not listed
    assert "out/side.txt" not in [entry.path for entry in entries]
    # the start file itself is never listed
    assert "src/core.py" not in [entry.path for entry in entries]


def test_chains_are_shortest_and_ties_break_lexicographically():
    stamp = stamp_of(DIAMOND)
    entries = recall_walk(stamp, full_state(stamp), "src/core.py")
    by_path = {entry.path: entry for entry in entries}
    assert by_path["gen/left.txt"].chain == ("src/core.py", "gen/left.txt")
    # both routes to out/final.txt have two edges, and the route through
    # gen/left.txt is the lexicographically smaller sequence of paths
    assert by_path["out/final.txt"].chain == (
        "src/core.py",
        "gen/left.txt",
        "out/final.txt",
    )


def test_a_longer_chain_is_followed_through_artifacts():
    body = (
        "file src/a.py created\ncontent\na\nend\n"
        "artifact gen/one.txt derived-from src/a.py class reversible\ncontent\n1\nend\n"
        "artifact gen/two.txt derived-from gen/one.txt class reversible\ncontent\n2\nend\n"
        "artifact gen/three.txt derived-from gen/two.txt class reversible\ncontent\n3\nend\n"
    )
    stamp = stamp_of(body)
    entries = recall_walk(stamp, full_state(stamp), "src/a.py")
    by_path = {entry.path: entry for entry in entries}
    assert by_path["gen/three.txt"].chain == (
        "src/a.py",
        "gen/one.txt",
        "gen/two.txt",
        "gen/three.txt",
    )


def test_sorting_is_by_class_first_and_then_by_path():
    body = (
        "file src/a.py created\ncontent\na\nend\n"
        "artifact zzz-last.txt derived-from src/a.py class reversible\ncontent\n1\nend\n"
        "artifact aaa-first.txt derived-from src/a.py class irreversible\ncontent\n2\nend\n"
        "artifact mmm-mid.txt derived-from src/a.py class reversible-with-compensation\ncontent\n3\nend\n"
        "artifact bbb.txt derived-from src/a.py class reversible\ncontent\n4\nend\n"
    )
    stamp = stamp_of(body)
    entries = recall_walk(stamp, full_state(stamp), "src/a.py")
    assert [entry.path for entry in entries] == [
        "bbb.txt",
        "zzz-last.txt",
        "mmm-mid.txt",
        "aaa-first.txt",
    ]
    assert [entry.rev_class for entry in entries] == [
        "reversible",
        "reversible",
        "reversible-with-compensation",
        "irreversible",
    ]


def test_statuses_report_current_drifted_and_missing():
    stamp = stamp_of(DIAMOND)
    state = full_state(stamp)
    state["gen/right.txt"] = "0" * 64
    del state["out/final.txt"]
    entries = recall_walk(stamp, state, "src/core.py")
    by_path = {entry.path: entry for entry in entries}
    assert by_path["gen/left.txt"].status == "current"
    assert by_path["gen/right.txt"].status == "drifted"
    assert by_path["out/final.txt"].status == "missing"


def test_the_walk_starts_only_from_a_stamped_file():
    stamp = stamp_of(DIAMOND)
    with pytest.raises(WalkError):
        recall_walk(stamp, full_state(stamp), "gen/left.txt")
    with pytest.raises(WalkError):
        recall_walk(stamp, full_state(stamp), "src/ghost.py")


def test_a_file_with_no_downstream_artifacts_walks_to_an_empty_result():
    stamp = stamp_of(DIAMOND)
    entries = recall_walk(stamp, full_state(stamp), "src/other.py")
    assert [entry.path for entry in entries] == ["out/side.txt"]
    body = "file src/lonely.py created\ncontent\nx\nend\n"
    lonely = stamp_of(body)
    assert recall_walk(lonely, {}, "src/lonely.py") == ()
    assert format_recall("src/lonely.py", ()) == "recall src/lonely.py\n"


def test_the_recall_text_names_artifact_class_status_and_chain():
    stamp = stamp_of(DIAMOND)
    entries = recall_walk(stamp, full_state(stamp), "src/core.py")
    text = format_recall("src/core.py", entries)
    lines = text.splitlines()
    assert lines[0] == "recall src/core.py"
    assert lines[1] == (
        "artifact gen/left.txt reversible current "
        "chain src/core.py -> gen/left.txt"
    )
    assert lines[3] == (
        "artifact out/final.txt irreversible current "
        "chain src/core.py -> gen/left.txt -> out/final.txt"
    )


def test_a_parsed_stamp_walks_the_same_as_a_built_one():
    stamp = stamp_of(DIAMOND)
    from badbatch.stamps import format_stamp

    parsed = parse_stamp(format_stamp(stamp), "stamp.txt")
    assert recall_walk(parsed, full_state(stamp), "src/core.py") == recall_walk(
        stamp, full_state(stamp), "src/core.py"
    )
