//! BB-23. Hardening completion and the stated readings.
//! Ported test for test in substance from
//! tests/test_hardening.py at the Round 4 closure.
//!
//! The Python originals put an integer key beside a string key
//! in dicts. The crate's value model keys objects by strings
//! only, so a mixed type key set cannot be constructed here at
//! all. That is the crash class BB-23 closed, removed by
//! construction. The ports below use string keys throughout
//! and pin the same outcomes. Where the Python named the first
//! extra field by printed form across types, the port pins the
//! printed form order across string keys.

mod common;

use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;

use badbatch::errors::Error;
use badbatch::judgment::{FixturePlayer, JudgmentSource, check_answer, trusted, validate_fixture};
use badbatch::pyjson::Value;
use badbatch::pytext::PyStr;
use badbatch::records::parse_run_record;
use badbatch::stamps::{build_stamp, format_stamp, parse_stamp};
use badbatch::walk::{format_recall, recall_walk};

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

fn s(text: &str) -> Value {
    Value::Str(py(text))
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (py(k), v)).collect())
}

fn probabilities() -> Value {
    obj(vec![
        ("reversible", Value::Float(0.9)),
        ("reversible-with-compensation", Value::Float(0.05)),
        ("irreversible", Value::Float(0.05)),
    ])
}

fn extra_key_answer() -> Value {
    let mut answer = obj(vec![
        ("kind", s("choice")),
        ("choice", s("reversible")),
        ("confidence", Value::Float(0.9)),
        ("probabilities", probabilities()),
    ]);
    if let Value::Object(pairs) = &mut answer {
        pairs.push((py("7"), s("a numeric string key")));
        pairs.push((py("note"), s("a string key")));
    }
    answer
}

struct StubSource {
    answer: Option<Value>,
}

impl JudgmentSource for StubSource {
    fn judge(&self, _request: &Value) -> Option<Value> {
        self.answer.clone()
    }
    fn judge_ranking(&self, _request: &Value) -> Option<Value> {
        None
    }
}

const HEADER: &str = "run run-0021\nagent writer-3\nprompt-hash c07ffee\nmodel synth-writer-2\n";
const RECORD_BODY: &str = "file src/report.py created\ncontent\nprint(\"report\")\nend\n\
artifact build/report.html derived-from src/report.py class unclassed\ncontent\n<html>report</html>\nend\n";

// The response ordering rule, BB-23 first rule.

#[test]
fn an_answer_with_extra_keys_counts_as_no_judgment() {
    let answer = extra_key_answer();
    assert_eq!(check_answer(Some(answer.clone())), None);
    assert!(!trusted(check_answer(Some(answer.clone())).as_ref()));
}

#[test]
fn a_walk_judged_by_an_extra_key_answer_refuses_as_with_no_source() {
    let record = parse_run_record(
        &py(&format!("{HEADER}{RECORD_BODY}")),
        &py("record.txt"),
    )
    .expect("record");
    let stamp = parse_stamp(
        &py(&format_stamp(&build_stamp(&record))),
        &py("stamp.txt"),
    )
    .expect("stamp");
    let state: HashMap<PyStr, PyStr> = stamp
        .artifacts
        .iter()
        .map(|e| (e.path.clone(), e.digest.clone()))
        .collect();
    let expected = "stamp.txt is malformed, the artifact line for \
build/report.html names the class 'unclassed', which is not one of the three named classes";
    let err = recall_walk(&stamp, &state, &py("src/report.py"), None)
        .expect_err("no source refuses");
    assert_eq!(err.message(), expected);
    let stub = StubSource {
        answer: Some(extra_key_answer()),
    };
    let err = recall_walk(&stamp, &state, &py("src/report.py"), Some(&stub))
        .expect_err("extra key source refuses the same way");
    assert_eq!(err.message(), expected);
}

#[test]
fn an_envelope_with_extra_keys_names_the_first_by_printed_form() {
    let mut pair = obj(vec![
        ("synthetic", Value::Bool(true)),
        ("judgment_point", s("classification")),
        (
            "request",
            obj(vec![
                ("artifact", s("build/report.html")),
                ("parents", Value::Array(vec![s("src/report.py")])),
                ("digest", s(&"0".repeat(64))),
                ("status", s("current")),
            ]),
        ),
        ("response", obj(vec![("kind", s("abstention"))])),
    ]);
    if let Value::Object(pairs) = &mut pair {
        pairs.push((py("zebra"), s("a later key by printed form")));
        pairs.push((py("note"), s("an earlier key by printed form")));
    }
    let err = validate_fixture(&pair, "fixture").expect_err("extra fields refused");
    assert_eq!(
        err.message(),
        "fixture carries the field 'note', which the fixture envelope does not carry"
    );
}

// The stamp readings, BB-23 second rule.

const STAMP_HEADER: &str =
    "stamp run-0009\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n";

#[test]
fn a_cyclic_stamp_is_admitted_and_the_walk_terminates_in_order() {
    let stamp = parse_stamp(
        &py(&format!(
            "{STAMP_HEADER}file src/seed.txt created {}\n\
artifact out/a.html reversible {} derived-from src/seed.txt, out/b.html\n\
artifact out/b.html irreversible {} derived-from out/a.html\n",
            "c".repeat(64),
            "a".repeat(64),
            "b".repeat(64)
        )),
        &py("stamp.txt"),
    )
    .expect("cyclic stamp admitted");
    let state: HashMap<PyStr, PyStr> = HashMap::from([
        (py("out/a.html"), py(&"a".repeat(64))),
        (py("out/b.html"), py(&"b".repeat(64))),
    ]);
    let entries = recall_walk(&stamp, &state, &py("src/seed.txt"), None).expect("walk");
    assert_eq!(
        format_recall(&py("src/seed.txt"), &entries),
        "recall src/seed.txt\n\
artifact out/a.html reversible current chain src/seed.txt -> out/a.html\n\
artifact out/b.html irreversible current chain src/seed.txt -> out/a.html -> out/b.html\n"
    );
}

#[test]
fn a_deleted_file_line_carrying_a_digest_is_admitted_and_kept() {
    let digest = "d".repeat(64);
    let with_digest = parse_stamp(
        &py(&format!(
            "{STAMP_HEADER}file src/old.txt deleted {digest}\n\
artifact out/a.html reversible {} derived-from src/old.txt\n",
            "a".repeat(64)
        )),
        &py("stamp.txt"),
    )
    .expect("with digest");
    let with_none = parse_stamp(
        &py(&format!(
            "{STAMP_HEADER}file src/old.txt deleted none\n\
artifact out/a.html reversible {} derived-from src/old.txt\n",
            "a".repeat(64)
        )),
        &py("stamp.txt"),
    )
    .expect("with none");
    assert_eq!(with_digest.files[0].digest, Some(py(&digest)));
    assert_eq!(with_none.files[0].digest, None);
    assert!(format_stamp(&with_digest)
        .lines()
        .any(|line| line == format!("file src/old.txt deleted {digest}")));
    let state: HashMap<PyStr, PyStr> =
        HashMap::from([(py("out/a.html"), py(&"a".repeat(64)))]);
    assert_eq!(
        recall_walk(&with_digest, &state, &py("src/old.txt"), None).expect("walk"),
        recall_walk(&with_none, &state, &py("src/old.txt"), None).expect("walk")
    );
}

// The duplicate key reading, BB-23 third rule.

fn fixture_text(first_kind: &str, last_kind: &str) -> String {
    format!(
        "{{\"synthetic\": true, \"judgment_point\": \"classification\", \
\"request\": {{\"artifact\": \"build/report.html\", \
\"parents\": [\"src/report.py\"], \"digest\": \"{}\", \
\"status\": \"current\"}}, \
\"response\": {{\"kind\": \"{first_kind}\", \"kind\": \"{last_kind}\", \
\"choice\": \"reversible\", \"confidence\": 0.9, \
\"probabilities\": {{\"reversible\": 0.9, \
\"reversible-with-compensation\": 0.05, \"irreversible\": 0.05}}}}}}",
        "0".repeat(64)
    )
}

#[test]
fn a_fixture_with_a_duplicated_kind_field_is_judged_on_the_last_kind() {
    let dir = common::test_dir("hardening-duplicate");
    common::write(&dir, "duplicated.json", &fixture_text("score", "choice"));
    let player = FixturePlayer::load(dir.as_os_str().as_bytes()).expect("player");
    let request = obj(vec![
        ("artifact", s("build/report.html")),
        ("parents", Value::Array(vec![s("src/report.py")])),
        ("digest", s(&"0".repeat(64))),
        ("status", s("current")),
    ]);
    let answer = player.judge(&request).expect("an answer");
    assert_eq!(answer.get("kind"), Some(&s("choice")));
    assert!(trusted(Some(&answer)));
}

#[test]
fn a_fixture_whose_last_duplicated_kind_is_invalid_is_refused() {
    let dir = common::test_dir("hardening-duplicate-bad");
    common::write(&dir, "duplicated.json", &fixture_text("choice", "score"));
    let err = FixturePlayer::load(dir.as_os_str().as_bytes())
        .expect_err("last kind score at the classification point refuses");
    assert!(matches!(err, Error::Judgment(_)), "{err:?}");
}
