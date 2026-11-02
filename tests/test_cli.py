import pathlib
"""BB-11, BB-12, and BB-13. The command line tool and its discipline."""

import subprocess
import sys

from badbatch.cli import main
from badbatch.records import parse_run_record
from badbatch.stamps import build_stamp, format_stamp

RECORD = """run run-0007
agent writer-3
prompt-hash b41d90
model synth-writer-2

file src/report.py created
content
print("report")
end

file src/legacy.py deleted

artifact build/report.html derived-from src/report.py class reversible
content
<html>report</html>
end
"""


def write(tmp_path, name, text):
    path = tmp_path / name
    path.write_text(text, encoding="utf-8")
    return str(path)


def stamp_file(tmp_path, record_text=RECORD):
    record = parse_run_record(record_text, "record.txt")
    return write(tmp_path, "stamp.txt", format_stamp(build_stamp(record)))


def test_stamp_command_prints_the_stamp_and_exits_zero(tmp_path, capsys):
    record = write(tmp_path, "record.txt", RECORD)
    assert main(["stamp", record]) == 0
    out = capsys.readouterr().out
    assert out.startswith("stamp run-0007\nagent writer-3\n")
    assert "file src/legacy.py deleted none\n" in out


def test_recall_command_prints_the_walk(tmp_path, capsys):
    stamp = stamp_file(tmp_path)
    state = write(
        tmp_path,
        "state.txt",
        "file build/report.html "
        + build_stamp(parse_run_record(RECORD, "record.txt")).artifacts[0].digest
        + "\n",
    )
    assert main(["recall", stamp, state, "src/report.py"]) == 0
    out = capsys.readouterr().out
    assert out == (
        "recall src/report.py\n"
        "artifact build/report.html reversible current "
        "chain src/report.py -> build/report.html\n"
    )


def test_a_malformed_record_exits_two_with_one_error_line(tmp_path, capsys):
    record = write(tmp_path, "record.txt", "run run-0007\nagent writer-3\n")
    assert main(["stamp", record]) == 2
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err.startswith("error: ")
    assert captured.err.count("\n") == 1
    assert "Traceback" not in captured.err


def test_a_missing_input_file_exits_two(tmp_path, capsys):
    assert main(["stamp", str(tmp_path / "ghost.txt")]) == 2
    captured = capsys.readouterr()
    assert captured.out == ""
    assert "cannot read" in captured.err


def test_recall_from_a_non_file_start_exits_two(tmp_path, capsys):
    stamp = stamp_file(tmp_path)
    state = write(tmp_path, "state.txt", "")
    assert main(["recall", stamp, state, "build/report.html"]) == 2
    assert capsys.readouterr().out == ""


def test_outputs_are_byte_identical_across_runs_and_input_order(
    tmp_path, capsys
):
    shuffled = RECORD.replace(
        "file src/report.py created\ncontent\nprint(\"report\")\nend\n\nfile src/legacy.py deleted",
        "file src/legacy.py deleted\n\nfile src/report.py created\ncontent\nprint(\"report\")\nend",
    )
    first = write(tmp_path, "record-a.txt", RECORD)
    second = write(tmp_path, "record-b.txt", shuffled)
    assert main(["stamp", first]) == 0
    out_first = capsys.readouterr().out
    assert main(["stamp", second]) == 0
    out_second = capsys.readouterr().out
    assert main(["stamp", first]) == 0
    out_third = capsys.readouterr().out
    assert out_first == out_second == out_third


def test_the_commands_modify_none_of_their_inputs(tmp_path, capsys):
    record = write(tmp_path, "record.txt", RECORD)
    stamp = stamp_file(tmp_path)
    state = write(tmp_path, "state.txt", "")
    before = {
        path: pathlib.Path(path).read_bytes() for path in (record, stamp, state)
    }
    assert main(["stamp", record]) == 0
    capsys.readouterr()
    assert main(["recall", stamp, state, "src/report.py"]) == 0
    capsys.readouterr()
    for path, content in before.items():
        assert pathlib.Path(path).read_bytes() == content


def test_a_recall_with_no_downstream_prints_the_header_alone(
    tmp_path, capsys
):
    record = write(
        tmp_path,
        "record.txt",
        RECORD.replace("artifact build/report.html", "artifact build/unrelated.html")
        .replace("derived-from src/report.py", "derived-from src/legacy.py"),
    )
    record_obj = parse_run_record(pathlib.Path(record).read_text(encoding="utf-8"), "record.txt")
    stamp = write(tmp_path, "stamp.txt", format_stamp(build_stamp(record_obj)))
    state = write(tmp_path, "state.txt", "")
    assert main(["recall", stamp, state, "src/report.py"]) == 0
    assert capsys.readouterr().out == "recall src/report.py\n"


def test_the_module_entry_point_runs(tmp_path):
    record = write(tmp_path, "record.txt", RECORD)
    result = subprocess.run(
        [sys.executable, "-m", "badbatch", "stamp", record],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0
    assert result.stdout.startswith("stamp run-0007\n")
    assert result.stderr == ""
