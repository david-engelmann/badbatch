"""The committed example session regenerates byte for byte."""

import pathlib

from badbatch.plan import build_plan, format_plan
from badbatch.records import parse_run_record
from badbatch.stamps import build_stamp, format_stamp, parse_stamp
from badbatch.walk import format_recall, parse_current_state, recall_walk

EXAMPLES = pathlib.Path(__file__).resolve().parents[1] / "examples"


def read(name):
    return (EXAMPLES / name).read_text(encoding="utf-8")


def test_the_example_stamp_regenerates_byte_for_byte():
    record = parse_run_record(read("run-record.txt"), "examples/run-record.txt")
    assert format_stamp(build_stamp(record)) == read("stamp.txt")


def test_the_example_recall_regenerates_byte_for_byte():
    stamp = parse_stamp(read("stamp.txt"), "examples/stamp.txt")
    state = parse_current_state(read("current-state.txt"), "examples/current-state.txt")
    entries = recall_walk(stamp, state, "src/report.py")
    assert format_recall("src/report.py", entries) == read("recall.txt")


def test_the_example_plan_regenerates_byte_for_byte():
    stamp = parse_stamp(read("stamp.txt"), "examples/stamp.txt")
    state = parse_current_state(read("current-state.txt"), "examples/current-state.txt")
    entries = build_plan(stamp, state, "src/report.py")
    assert format_plan("src/report.py", entries) == read("plan.txt")


def test_the_example_session_shows_drift_from_the_other_start_file():
    stamp = parse_stamp(read("stamp.txt"), "examples/stamp.txt")
    state = parse_current_state(read("current-state.txt"), "examples/current-state.txt")
    entries = recall_walk(stamp, state, "src/data.csv")
    by_path = {entry.path: entry for entry in entries}
    assert by_path["build/chart.png"].status == "drifted"
    assert by_path["dist/bundle.zip"].status == "current"
    assert by_path["build/chart.png"].chain == ("src/data.csv", "build/chart.png")
