"""BB-1 and BB-2. Run record parsing and malformed record refusals."""

import pytest

from badbatch.errors import RecordError
from badbatch.records import parse_run_record
from badbatch.stamps import build_stamp, format_stamp

HEADER = """run run-0007
agent writer-3
prompt-hash b41d90
model synth-writer-2
"""

FILE_BLOCK = """file src/report.py created
content
print("report")
end
"""

ARTIFACT_BLOCK = """artifact build/report.html derived-from src/report.py class reversible
content
<html>report</html>
end
"""


def record(body):
    return parse_run_record(HEADER + body, "record.txt")


def test_a_well_formed_record_parses_with_its_header_and_entries():
    parsed = record(FILE_BLOCK + "\nfile src/legacy.py deleted\n" + ARTIFACT_BLOCK)
    assert parsed.run == "run-0007"
    assert parsed.agent == "writer-3"
    assert parsed.prompt_hash == "b41d90"
    assert parsed.model == "synth-writer-2"
    assert [entry.path for entry in parsed.files] == [
        "src/report.py",
        "src/legacy.py",
    ]
    assert parsed.files[0].content == ('print("report")',)
    assert parsed.files[1].action == "deleted"
    assert parsed.files[1].content is None
    assert parsed.artifacts[0].parents == ("src/report.py",)
    assert parsed.artifacts[0].rev_class == "reversible"


def test_entries_may_appear_in_any_order_after_the_header():
    parsed = record(ARTIFACT_BLOCK + FILE_BLOCK)
    assert parsed.artifacts[0].path == "build/report.html"
    assert parsed.files[0].path == "src/report.py"


def test_comments_and_blank_lines_outside_content_are_ignored():
    parsed = record("# a note\n\n" + FILE_BLOCK + "\n# another\n")
    assert len(parsed.files) == 1


def test_content_lines_are_literal_including_blanks_and_hash_lines():
    body = (
        "file src/notes.txt created\n"
        "content\n"
        "first\n"
        "\n"
        "# not a comment\n"
        " end \n"
        "end\n"
    )
    parsed = record(body)
    assert parsed.files[0].content == ("first", "", "# not a comment", " end ")


@pytest.mark.parametrize(
    "text",
    [
        # the model header line is missing entirely
        "run run-0007\nagent writer-3\nprompt-hash b41d90\n",
        # the header lines are out of order
        "agent writer-3\nrun run-0007\nprompt-hash b41d90\nmodel synth-writer-2\n",
        # a header field is named twice in the header block
        "run run-0007\nagent writer-3\nagent writer-3\nmodel synth-writer-2\n",
        # the record opens with an entry instead of the header
        FILE_BLOCK + HEADER,
        # a header line carries two values
        HEADER.replace("agent writer-3", "agent writer-3 writer-4") + FILE_BLOCK,
    ],
)
def test_header_defects_are_refused(text):
    with pytest.raises(RecordError):
        parse_run_record(text, "record.txt")


@pytest.mark.parametrize(
    "body",
    [
        # a meaningful line that matches no entry form
        "frobnicate src/report.py\n",
        # an action outside created, modified, or deleted
        "file src/report.py moved\ncontent\nx\nend\n",
        # a created file with no content block after it
        "file src/report.py created\nfile src/other.py deleted\n",
        # a modified file reaching the end of the record with no block
        "file src/report.py modified\n",
        # a deleted file carrying a content block
        "file src/report.py deleted\ncontent\nx\nend\n",
        # a content block with no closing end line
        "file src/report.py created\ncontent\nx\n",
        # a duplicate file path
        FILE_BLOCK + FILE_BLOCK,
        # an absolute path
        "file /etc/passwd created\ncontent\nx\nend\n",
        # a path with a parent reference
        "file ../outside.py created\ncontent\nx\nend\n",
        # an artifact with an empty derivation list
        FILE_BLOCK + "artifact build/x.html derived-from class reversible\ncontent\nx\nend\n",
        # an artifact with no class
        FILE_BLOCK + "artifact build/x.html derived-from src/report.py\ncontent\nx\nend\n",
        # an artifact with a class outside the three named classes
        FILE_BLOCK + "artifact build/x.html derived-from src/report.py class forever\ncontent\nx\nend\n",
        # an artifact whose derivation names an undeclared path
        FILE_BLOCK + "artifact build/x.html derived-from src/ghost.py class reversible\ncontent\nx\nend\n",
        # an artifact with no content block
        FILE_BLOCK + "artifact build/x.html derived-from src/report.py class reversible\n",
        # a duplicate artifact path
        FILE_BLOCK + ARTIFACT_BLOCK + ARTIFACT_BLOCK,
        # an artifact path that equals a file path
        FILE_BLOCK + "artifact src/report.py derived-from src/report.py class reversible\ncontent\nx\nend\n",
        # an artifact that derives from itself
        FILE_BLOCK + "artifact build/x.html derived-from build/x.html class reversible\ncontent\nx\nend\n",
        # an indirect derivation cycle through two artifacts
        FILE_BLOCK
        + "artifact build/a.html derived-from build/b.html class reversible\ncontent\nx\nend\n"
        + "artifact build/b.html derived-from build/a.html class reversible\ncontent\nx\nend\n",
    ],
)
def test_malformed_records_are_refused(body):
    with pytest.raises(RecordError):
        record(body)


def test_a_refusal_names_the_source_and_the_defect():
    with pytest.raises(RecordError) as caught:
        record("file src/report.py moved\ncontent\nx\nend\n")
    message = str(caught.value)
    assert "record.txt" in message
    assert "moved" in message


def _deep_record(count, deepest_first):
    lines = ["run run-deep", "agent agent-s", "prompt-hash hash-s", "model model-s"]
    lines.append("file src/base.txt created")
    lines.append("content")
    lines.append("base")
    lines.append("end")
    order = list(range(count))
    if deepest_first:
        order.reverse()
    for i in order:
        parent = "src/base.txt" if i == 0 else f"gen/step-{i - 1}.txt"
        lines.append(f"artifact gen/step-{i}.txt derived-from {parent} class reversible")
        lines.append("content")
        lines.append(f"step {i}")
        lines.append("end")
    return "\n".join(lines) + "\n"


def test_a_deep_chain_listed_deepest_first_stamps():
    record = parse_run_record(_deep_record(1500, True), "deep.txt")
    stamp = format_stamp(build_stamp(record))
    assert "gen/step-1499.txt" in stamp


def test_a_deep_cycle_is_refused_with_the_chain_named():
    text = _deep_record(1200, True).replace(
        "derived-from src/base.txt", "derived-from gen/step-1199.txt"
    )
    with pytest.raises(RecordError):
        parse_run_record(text, "deep-cycle.txt")


def test_a_form_feed_inside_a_content_line_stays_one_line():
    text = (
        "run run-ff\nagent a\nprompt-hash h\nmodel m\n"
        "file f.txt created\ncontent\nal\x0cpha\nend\n"
    )
    record = parse_run_record(text, "ff.txt")
    stamp = format_stamp(build_stamp(record))
    import hashlib

    expect = hashlib.sha256("al\x0cpha\n".encode()).hexdigest()
    assert expect in stamp


def test_a_crlf_record_is_refused_rather_than_reinterpreted():
    text = (
        "run run-c\r\nagent a\r\nprompt-hash h\r\nmodel m\r\n"
        "file f.txt created\r\ncontent\r\nfoo\r\nend\r\n"
    )
    with pytest.raises(RecordError):
        parse_run_record(text, "crlf.txt")
