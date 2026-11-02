"""BB-3, BB-4, and BB-5. Stamp building, the digest rule, stamp parsing."""

import re

import pytest

from badbatch.errors import StampError
from badbatch.records import content_digest, parse_run_record
from badbatch.stamps import build_stamp, format_stamp, parse_stamp

HEADER = """run run-0007
agent writer-3
prompt-hash b41d90
model synth-writer-2
"""

# SHA-256 of the bytes alpha, newline, beta, newline, computed from the
# byte rule the contract states, not from the package under test.
ALPHA_BETA_DIGEST = "e49c81e2d2f84e259d40e2fb8192f3bcd198b355184845d76d8f58807d0d78ee"
EMPTY_DIGEST = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"


def stamp_text(body):
    record = parse_run_record(HEADER + body, "record.txt")
    return format_stamp(build_stamp(record))


def test_the_stamp_carries_provenance_files_and_artifacts_sorted():
    body = (
        "file src/zeta.py created\ncontent\nalpha\nbeta\nend\n"
        "file src/legacy.py deleted\n"
        "file src/alpha.py modified\ncontent\nalpha\nbeta\nend\n"
        "artifact build/z.html derived-from src/zeta.py, src/alpha.py class irreversible\n"
        "content\npage\nend\n"
    )
    text = stamp_text(body)
    lines = text.splitlines()
    assert lines[:4] == [
        "stamp run-0007",
        "agent writer-3",
        "prompt-hash b41d90",
        "model synth-writer-2",
    ]
    assert lines[4] == f"file src/alpha.py modified {ALPHA_BETA_DIGEST}"
    assert lines[5] == "file src/legacy.py deleted none"
    assert lines[6] == f"file src/zeta.py created {ALPHA_BETA_DIGEST}"
    artifact_line = lines[7]
    assert artifact_line.startswith("artifact build/z.html irreversible ")
    assert artifact_line.endswith("derived-from src/alpha.py, src/zeta.py")
    assert len(lines) == 8


def test_the_digest_rule_matches_the_stated_byte_rule():
    assert content_digest(("alpha", "beta")) == ALPHA_BETA_DIGEST
    assert content_digest(()) == EMPTY_DIGEST
    # one extra line is a different digest, so the rule is content exact
    assert content_digest(("alpha", "beta", "gamma")) != ALPHA_BETA_DIGEST
    # the same content under two paths digests the same in one stamp
    body = (
        "file a/one.txt created\ncontent\nalpha\nbeta\nend\n"
        "file b/two.txt created\ncontent\nalpha\nbeta\nend\n"
    )
    text = stamp_text(body)
    assert text.count(ALPHA_BETA_DIGEST) == 2


def test_every_printed_digest_is_sixty_four_lowercase_hex():
    body = (
        "file src/report.py created\ncontent\nprint(1)\nend\n"
        "artifact build/r.html derived-from src/report.py class reversible\n"
        "content\npage\nend\n"
    )
    for token in stamp_text(body).split():
        if len(token) == 64:
            assert re.fullmatch(r"[0-9a-f]{64}", token)


def test_a_stamp_round_trips_through_the_parser():
    body = (
        "file src/report.py created\ncontent\nalpha\nbeta\nend\n"
        "file src/legacy.py deleted\n"
        "artifact build/r.html derived-from src/report.py class reversible-with-compensation\n"
        "content\npage\nend\n"
    )
    record = parse_run_record(HEADER + body, "record.txt")
    stamp = build_stamp(record)
    parsed = parse_stamp(format_stamp(stamp), "stamp.txt")
    assert parsed.run == stamp.run
    assert parsed.agent == stamp.agent
    assert parsed.prompt_hash == stamp.prompt_hash
    assert parsed.model == stamp.model
    assert parsed.files == stamp.files
    assert parsed.artifacts == stamp.artifacts


@pytest.mark.parametrize(
    "text",
    [
        # the model provenance field is missing
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\n",
        # a created file line carrying none instead of a digest
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n"
        "file src/report.py created none\n",
        # a digest that is not sixty-four lowercase hex characters
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n"
        "file src/report.py created ABC123\n",
        # an artifact deriving from a path the stamp does not carry
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n"
        f"artifact build/r.html reversible {ALPHA_BETA_DIGEST} derived-from src/ghost.py\n",
        # the same path stamped as a file and as an artifact
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n"
        f"file build/r.html created {ALPHA_BETA_DIGEST}\n"
        f"artifact build/r.html reversible {ALPHA_BETA_DIGEST} derived-from build/r.html\n",
        # a line matching no stamp form
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n"
        "frobnicate build/r.html\n",
        # an artifact carrying none as its digest
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n"
        "file src/report.py created " + ALPHA_BETA_DIGEST + "\n"
        "artifact build/r.html reversible none derived-from src/report.py\n",
    ],
)
def test_malformed_stamps_are_refused(text):
    with pytest.raises(StampError):
        parse_stamp(text, "stamp.txt")


def test_a_deleted_file_line_may_carry_a_digest_in_a_foreign_stamp():
    text = (
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n"
        f"file src/legacy.py deleted {ALPHA_BETA_DIGEST}\n"
    )
    parsed = parse_stamp(text, "stamp.txt")
    assert parsed.files[0].digest == ALPHA_BETA_DIGEST
