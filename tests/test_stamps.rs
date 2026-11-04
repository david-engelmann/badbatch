//! The stamp build and parse, ported test for test in substance
//! from tests/test_stamps.py at the Round 4 closure.

use badbatch::errors::Error;
use badbatch::pytext::PyStr;
use badbatch::records::{content_digest, parse_run_record};
use badbatch::stamps::{build_stamp, format_stamp, parse_stamp};

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

const ALPHA_BETA_DIGEST: &str = "e49c81e2d2f84e259d40e2fb8192f3bcd198b355184845d76d8f58807d0d78ee";
const EMPTY_DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

const RECORD: &str = "run run-0013\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file zeta.txt created\ncontent\nalpha\nbeta\nend\n\
file alpha.txt modified\ncontent\nend\n\
file legacy.txt deleted\n\
artifact out/final.html derived-from zeta.txt, alpha.txt class irreversible\ncontent\n<html>final</html>\nend\n";

const STAMP_TEXT: &str = "stamp run-0013\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file alpha.txt modified e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n\
file legacy.txt deleted none\n\
file zeta.txt created e49c81e2d2f84e259d40e2fb8192f3bcd198b355184845d76d8f58807d0d78ee\n\
artifact out/final.html irreversible 5a9907b7889417ef1192d74555982542f6527b2616af44b6696f9236972fce6d derived-from alpha.txt, zeta.txt\n";

#[test]
fn the_stamp_layout_is_sorted_and_exact() {
    let record = parse_run_record(&py(RECORD), &py("record.txt")).expect("record");
    let stamp = build_stamp(&record);
    let text = format_stamp(&stamp);
    assert_eq!(text, STAMP_TEXT);
}

#[test]
fn content_digest_pins() {
    assert_eq!(
        content_digest(&[py("alpha"), py("beta")]),
        ALPHA_BETA_DIGEST
    );
    assert_eq!(content_digest(&[]), EMPTY_DIGEST);
}

#[test]
fn every_printed_digest_is_sixty_four_lowercase_hex() {
    let record = parse_run_record(&py(RECORD), &py("record.txt")).expect("record");
    let text = format_stamp(&build_stamp(&record));
    for token in text.split_whitespace() {
        if token.len() == 64 {
            assert!(
                token
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                "{token}"
            );
        }
    }
}

#[test]
fn a_stamp_round_trips() {
    let record = parse_run_record(&py(RECORD), &py("record.txt")).expect("record");
    let stamp = build_stamp(&record);
    let parsed = parse_stamp(&py(&format_stamp(&stamp)), &py("record.txt")).expect("round trip");
    assert_eq!(parsed, stamp);
}

const HEADER: &str = "stamp run-0013\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n";

#[test]
fn malformed_stamps_are_refused() {
    let digest = "a".repeat(64);
    let cases: Vec<String> = vec![
        // The model header line is missing.
        "stamp run-0013\nagent writer-3\nprompt-hash b41d90\n".to_string(),
        // A created file carrying the digest word none.
        format!("{HEADER}file src/a.txt created none\n"),
        // A digest that is not sixty four lowercase hex.
        format!("{HEADER}file src/a.txt created ABC123\n"),
        // An artifact deriving from a parent the stamp does not carry.
        format!(
            "{HEADER}file src/a.txt created {digest}\nartifact out/a.html reversible {digest} derived-from src/ghost.txt\n"
        ),
        // The same path stamped as a file and as an artifact.
        format!(
            "{HEADER}file src/a.txt created {digest}\nartifact src/a.txt reversible {digest} derived-from src/a.txt\n"
        ),
        // A line matching no stamp line form.
        format!("{HEADER}something else entirely\n"),
        // An artifact carrying the digest word none.
        format!(
            "{HEADER}file src/a.txt created {digest}\nartifact out/a.html reversible none derived-from src/a.txt\n"
        ),
    ];
    for text in &cases {
        let err = parse_stamp(&py(text), &py("stamp.txt")).expect_err("malformed stamp refused");
        assert!(matches!(err, Error::Stamp(_)), "{err:?} for {text}");
    }
}

#[test]
fn a_deleted_file_line_may_carry_a_digest_and_keeps_it() {
    // BB-23 second rule. A foreign stamp is read as written.
    let digest = "d".repeat(64);
    let a_digest = "a".repeat(64);
    let stamp = parse_stamp(
        &py(&format!(
            "{HEADER}file src/old.txt deleted {digest}\nartifact out/a.html reversible {a_digest} derived-from src/old.txt\n"
        )),
        &py("stamp.txt"),
    )
    .expect("deleted with digest admitted");
    assert_eq!(stamp.files[0].digest, Some(py(&digest)));
    assert!(format_stamp(&stamp).contains(&format!("file src/old.txt deleted {digest}")));
}
