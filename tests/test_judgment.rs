//! The classification seam, ported test for test in substance
//! from tests/test_judgment.py at the Round 4 closure.
//!
//! Tests the Python ran in process against main() run the
//! compiled binary here, per BB-40's cargo only suite rule.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use badbatch::errors::Error;
use badbatch::judgment::{
    FixturePlayer, JudgmentSource, check_answer, classification_request, trusted,
    validate_fixture,
};
use badbatch::pyjson::Value;
use badbatch::pytext::PyStr;
use badbatch::records::parse_run_record;
use badbatch::stamps::{Stamp, StampArtifact, build_stamp, format_stamp, parse_stamp};
use badbatch::walk::{format_recall, recall_walk};

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

fn s(text: &str) -> Value {
    Value::Str(py(text))
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(
        pairs
            .into_iter()
            .map(|(k, v)| (py(k), v))
            .collect(),
    )
}

fn fixtures_dir() -> String {
    format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"))
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

const HEADER: &str = "run run-0021\nagent writer-3\nprompt-hash c07ffee\nmodel synth-writer-2\n";
const FILES: &str = "file src/report.py created\ncontent\nprint(\"report\")\nend\nfile src/data.csv created\ncontent\na,b\nend\n";
const ART_HTML: &str = "artifact build/report.html derived-from src/report.py class unclassed\ncontent\n<html>report</html>\nend\n";
const ART_SUMMARY: &str = "artifact docs/summary.md derived-from build/report.html class unclassed\ncontent\nsummary\nend\n";
const ART_BUNDLE: &str = "artifact dist/bundle.zip derived-from src/data.csv class unclassed\ncontent\nbundle\nend\n";
const ART_CHART: &str = "artifact build/chart.png derived-from src/data.csv class unclassed\ncontent\nchart\nend\n";
const ART_PDF: &str = "artifact exports/report.pdf derived-from docs/summary.md class unclassed\ncontent\npdf\nend\n";
const ART_PAGE: &str = "artifact build/page.html derived-from src/report.py class irreversible\ncontent\npage\nend\n";
const ART_ZPAGE: &str = "artifact z/page.txt derived-from src/data.csv class reversible\ncontent\nzpage\nend\n";

fn probabilities() -> Value {
    obj(vec![
        ("reversible", Value::Float(0.9)),
        ("reversible-with-compensation", Value::Float(0.05)),
        ("irreversible", Value::Float(0.05)),
    ])
}

fn choice_answer(confidence: f64) -> Value {
    obj(vec![
        ("kind", s("choice")),
        ("choice", s("reversible")),
        ("confidence", Value::Float(confidence)),
        ("probabilities", probabilities()),
    ])
}

fn stamp_of(body: &str) -> Stamp {
    let record = parse_run_record(&py(&format!("{HEADER}{body}")), &py("record.txt"))
        .expect("record parses");
    parse_stamp(
        &py(&format_stamp(&build_stamp(&record))),
        &py("stamp.txt"),
    )
    .expect("stamp parses")
}

fn state_for(
    stamp: &Stamp,
    drifted: &[&str],
    missing: &[&str],
) -> HashMap<PyStr, PyStr> {
    let mut state = HashMap::new();
    for entry in &stamp.artifacts {
        if missing.iter().any(|m| py(m) == entry.path) {
            continue;
        }
        let digest = if drifted.iter().any(|m| py(m) == entry.path) {
            py(&"0".repeat(64))
        } else {
            entry.digest.clone()
        };
        state.insert(entry.path.clone(), digest);
    }
    state
}

fn refusal_line(path: &str, source: &str) -> String {
    format!(
        "{source} is malformed, the artifact line for {path} names the class \
         'unclassed', which is not one of the three named classes"
    )
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

struct SpyPlayer {
    inner: FixturePlayer,
    requests: RefCell<Vec<Value>>,
}

impl JudgmentSource for SpyPlayer {
    fn judge(&self, request: &Value) -> Option<Value> {
        self.requests.borrow_mut().push(request.clone());
        self.inner.judge(request)
    }
    fn judge_ranking(&self, request: &Value) -> Option<Value> {
        self.inner.judge_ranking(request)
    }
}

// The unclassed marker, BB-16.

#[test]
fn the_marker_round_trips_from_record_to_stamp() {
    let record =
        parse_run_record(&py(&format!("{HEADER}{FILES}{ART_HTML}")), &py("record.txt"))
            .expect("record");
    let stamp = build_stamp(&record);
    assert_eq!(stamp.artifacts[0].rev_class, py("unclassed"));
    let text = format_stamp(&stamp);
    assert!(text.contains("artifact build/report.html unclassed "), "{text}");
    let parsed = parse_stamp(&py(&text), &py("stamp.txt")).expect("stamp");
    assert_eq!(parsed.artifacts[0].rev_class, py("unclassed"));
}

#[test]
fn a_class_word_outside_the_three_and_the_marker_is_refused() {
    let body = ART_HTML.replace("class unclassed", "class sideways");
    let err = parse_run_record(&py(&format!("{HEADER}{FILES}{body}")), &py("record.txt"))
        .expect_err("record refuses sideways");
    assert!(matches!(err, Error::Record(_)), "{err:?}");
    let stamp_text = format_stamp(&stamp_of(&format!("{FILES}{ART_HTML}")));
    let bad = stamp_text.replace(" unclassed ", " sideways ");
    let err = parse_stamp(&py(&bad), &py("stamp.txt")).expect_err("stamp refuses");
    assert_eq!(
        err.message(),
        "stamp.txt is malformed, the artifact line for build/report.html \
         names the class 'sideways', which is not one of the three named classes"
    );
}

// The recorded fixture shape, BB-17 and BB-18.

#[test]
fn every_committed_fixture_matches_the_recorded_shape() {
    let dir = root().join("fixtures");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("read fixtures dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 5);
    for name in &names {
        let text = std::fs::read_to_string(dir.join(name)).expect("read fixture");
        let pair = badbatch::pyjson::loads(&py(&text)).expect("fixture parses");
        let validated = validate_fixture(&pair, name).expect("fixture validates");
        assert_eq!(validated, pair);
    }
}

#[test]
fn the_player_answers_an_exact_match_and_abstains_otherwise() {
    let player = FixturePlayer::load(fixtures_dir().as_bytes()).expect("player loads");
    let artifact = StampArtifact {
        path: py("build/report.html"),
        rev_class: py("unclassed"),
        digest: py(&"2".repeat(64)),
        parents: vec![py("src/report.py")],
    };
    let request = classification_request(&artifact, "current");
    assert_eq!(player.judge(&request), None);
    let text = std::fs::read_to_string(
        root().join("fixtures/classification-report-html-reversible.json"),
    )
    .expect("read committed fixture");
    let committed = badbatch::pyjson::loads(&py(&text)).expect("parses");
    let committed_request = committed.get("request").expect("request").clone();
    let committed_response = committed.get("response").expect("response").clone();
    assert_eq!(player.judge(&committed_request), Some(committed_response));
}

fn good_pair() -> Value {
    obj(vec![
        ("judgment_point", s("classification")),
        (
            "request",
            obj(vec![
                ("artifact", s("build/report.html")),
                (
                    "parents",
                    Value::Array(vec![s("src/report.py")]),
                ),
                ("digest", s(&"2".repeat(64))),
                ("status", s("current")),
            ]),
        ),
        ("response", choice_answer(0.9)),
        ("synthetic", Value::Bool(true)),
    ])
}

fn set_field(value: &mut Value, key: &str, replacement: Value) {
    if let Value::Object(pairs) = value {
        for (name, slot) in pairs.iter_mut() {
            if *name == py(key) {
                *slot = replacement;
                return;
            }
        }
        pairs.push((py(key), replacement));
    }
}

fn remove_field(value: &mut Value, key: &str) {
    if let Value::Object(pairs) = value {
        pairs.retain(|(name, _)| *name != py(key));
    }
}

fn field_mut<'a>(value: &'a mut Value, key: &str) -> &'a mut Value {
    match value {
        Value::Object(pairs) => pairs
            .iter_mut()
            .find(|(name, _)| *name == py(key))
            .map(|(_, slot)| slot)
            .expect("field present"),
        _ => panic!("not an object"),
    }
}

fn write_fixture(dir: &Path, name: &str, pair: &Value) {
    let mut text = String::new();
    badbatch::pyjson::dumps_canonical(pair, &mut text);
    common::write(dir, name, &text);
}

fn mutated_pairs() -> Vec<Value> {
    let mut out = Vec::new();
    let mut pair = good_pair();
    set_field(&mut pair, "synthetic", Value::Bool(false));
    out.push(pair);
    let mut pair = good_pair();
    set_field(&mut pair, "judgment_point", s("severity"));
    out.push(pair);
    let mut pair = good_pair();
    set_field(field_mut(&mut pair, "response"), "note", s("extra"));
    out.push(pair);
    let mut pair = good_pair();
    let response = field_mut(&mut pair, "response");
    set_field(response, "kind", s("abstention"));
    set_field(response, "confidence", Value::Float(0.9));
    out.push(pair);
    let mut pair = good_pair();
    remove_field(field_mut(&mut pair, "response"), "probabilities");
    out.push(pair);
    let mut pair = good_pair();
    remove_field(
        field_mut(field_mut(&mut pair, "response"), "probabilities"),
        "irreversible",
    );
    out.push(pair);
    let mut pair = good_pair();
    let probs = field_mut(field_mut(&mut pair, "response"), "probabilities");
    set_field(probs, "reversible", Value::Float(0.5));
    set_field(probs, "reversible-with-compensation", Value::Float(0.0));
    out.push(pair);
    let mut pair = good_pair();
    set_field(
        field_mut(&mut pair, "response"),
        "confidence",
        Value::Float(1.5),
    );
    out.push(pair);
    let mut pair = good_pair();
    remove_field(field_mut(&mut pair, "request"), "status");
    out.push(pair);
    let mut pair = good_pair();
    set_field(field_mut(&mut pair, "request"), "extra", s("field"));
    out.push(pair);
    let mut pair = good_pair();
    set_field(field_mut(&mut pair, "request"), "digest", s("not-hex"));
    out.push(pair);
    out
}

#[test]
fn a_fixture_outside_the_shape_is_refused_at_load() {
    for (index, pair) in mutated_pairs().iter().enumerate() {
        let dir = common::test_dir("judgment-mutation");
        write_fixture(&dir, "bad.json", pair);
        let err = FixturePlayer::load(dir.as_os_str().as_bytes())
            .expect_err("mutation refused");
        assert!(matches!(err, Error::Judgment(_)), "case {index}: {err:?}");
    }
}

#[test]
fn a_fixture_that_is_not_json_is_refused_at_load() {
    let dir = common::test_dir("judgment-notjson");
    common::write(&dir, "bad.json", "this is not json\n");
    let err = FixturePlayer::load(dir.as_os_str().as_bytes())
        .expect_err("not json refused");
    assert!(matches!(err, Error::Judgment(_)), "{err:?}");
}

#[test]
fn two_fixtures_for_one_request_are_refused_at_load() {
    let dir = common::test_dir("judgment-duplicate");
    write_fixture(&dir, "one.json", &good_pair());
    write_fixture(&dir, "two.json", &good_pair());
    let err = FixturePlayer::load(dir.as_os_str().as_bytes())
        .expect_err("duplicate refused");
    assert!(matches!(err, Error::Judgment(_)), "{err:?}");
}

#[test]
fn a_missing_fixtures_directory_is_refused_at_load() {
    let dir = common::test_dir("judgment-missing");
    let ghost = dir.join("ghost");
    let err = FixturePlayer::load(ghost.as_os_str().as_bytes())
        .expect_err("missing dir refused");
    assert!(matches!(err, Error::Judgment(_)), "{err:?}");
}

// Answers from any source, BB-19 and BB-20.

#[test]
fn a_malformed_foreign_answer_counts_as_no_judgment() {
    let mut missing_probs = choice_answer(0.9);
    remove_field(&mut missing_probs, "probabilities");
    let mut extra = choice_answer(0.9);
    set_field(&mut extra, "note", s("extra"));
    let answers: Vec<Value> = vec![
        obj(vec![
            ("kind", s("choice")),
            ("choice", s("reversible")),
            ("confidence", Value::Float(0.9)),
        ]),
        extra,
        obj(vec![
            ("kind", s("score")),
            ("score", Value::Float(0.9)),
            ("confidence", Value::Float(0.9)),
        ]),
        obj(vec![
            ("kind", s("choice")),
            ("choice", s("reversible")),
            ("confidence", Value::Float(0.9)),
            (
                "probabilities",
                obj(vec![("reversible", Value::Float(1.0))]),
            ),
        ]),
        s("not an answer at all"),
    ];
    for answer in &answers {
        assert_eq!(check_answer(Some(answer.clone())), None);
        assert!(!trusted(check_answer(Some(answer.clone())).as_ref()));
    }
}

#[test]
fn an_abstention_is_well_formed_but_never_trusted() {
    let answer = obj(vec![("kind", s("abstention"))]);
    assert_eq!(check_answer(Some(answer.clone())), Some(answer.clone()));
    assert!(!trusted(Some(&answer)));
}

#[test]
fn the_trust_threshold_edge_is_exactly_point_six() {
    assert!(trusted(Some(&choice_answer(0.6))));
    assert!(!trusted(Some(&choice_answer(0.599999))));
}

// The walk rule, BB-21.

#[test]
fn without_a_source_an_unclassed_artifact_refuses_as_round_1() {
    let stamp = stamp_of(&format!("{FILES}{ART_HTML}{ART_SUMMARY}"));
    let err = recall_walk(&stamp, &state_for(&stamp, &[], &[]), &py("src/report.py"), None)
        .expect_err("unclassed refuses");
    assert_eq!(err.message(), refusal_line("build/report.html", "stamp.txt"));
}

#[test]
fn trusted_proposals_class_the_walk_with_the_marker() {
    let stamp = stamp_of(&format!("{FILES}{ART_HTML}{ART_SUMMARY}{ART_BUNDLE}"));
    let player = FixturePlayer::load(fixtures_dir().as_bytes()).expect("player");
    let state = state_for(&stamp, &["docs/summary.md"], &[]);
    let entries = recall_walk(&stamp, &state, &py("src/report.py"), Some(&player))
        .expect("walk with proposals");
    let summary: Vec<(String, String, bool)> = entries
        .iter()
        .map(|e| (e.path.to_string(), e.rev_class.to_string(), e.proposed))
        .collect();
    assert_eq!(
        summary,
        [
            ("build/report.html".to_string(), "reversible".to_string(), true),
            (
                "docs/summary.md".to_string(),
                "reversible-with-compensation".to_string(),
                true
            ),
        ]
    );
    assert_eq!(
        format_recall(&py("src/report.py"), &entries),
        "recall src/report.py\n\
artifact build/report.html reversible proposed current chain src/report.py -> build/report.html\n\
artifact docs/summary.md reversible-with-compensation proposed drifted chain src/report.py -> build/report.html -> docs/summary.md\n"
    );
    let state = state_for(&stamp, &[], &["dist/bundle.zip"]);
    let entries = recall_walk(&stamp, &state, &py("src/data.csv"), Some(&player))
        .expect("walk from data");
    let summary: Vec<(String, String, String, bool)> = entries
        .iter()
        .map(|e| {
            (
                e.path.to_string(),
                e.rev_class.to_string(),
                e.status.to_string(),
                e.proposed,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [(
            "dist/bundle.zip".to_string(),
            "irreversible".to_string(),
            "missing".to_string(),
            true
        )]
    );
}

#[test]
fn a_proposed_class_sorts_into_its_proposed_group() {
    let stamp = stamp_of(&format!("{FILES}{ART_BUNDLE}{ART_ZPAGE}"));
    let player = FixturePlayer::load(fixtures_dir().as_bytes()).expect("player");
    let state = state_for(&stamp, &[], &["dist/bundle.zip"]);
    let entries = recall_walk(&stamp, &state, &py("src/data.csv"), Some(&player))
        .expect("walk");
    let paths: Vec<String> = entries.iter().map(|e| e.path.to_string()).collect();
    assert_eq!(paths, ["z/page.txt", "dist/bundle.zip"]);
    let proposed: Vec<bool> = entries.iter().map(|e| e.proposed).collect();
    assert_eq!(proposed, [false, true]);
}

#[test]
fn an_abstention_refuses_the_walk_naming_the_artifact() {
    let stamp = stamp_of(&format!("{FILES}{ART_BUNDLE}{ART_CHART}"));
    let player = FixturePlayer::load(fixtures_dir().as_bytes()).expect("player");
    let err = recall_walk(
        &stamp,
        &state_for(&stamp, &[], &[]),
        &py("src/data.csv"),
        Some(&player),
    )
    .expect_err("abstention refuses");
    assert_eq!(err.message(), refusal_line("build/chart.png", "stamp.txt"));
}

#[test]
fn a_below_threshold_answer_refuses_the_walk() {
    let stamp = stamp_of(&format!("{FILES}{ART_HTML}{ART_SUMMARY}{ART_PDF}"));
    let player = FixturePlayer::load(fixtures_dir().as_bytes()).expect("player");
    let state = state_for(&stamp, &["docs/summary.md"], &[]);
    let err = recall_walk(&stamp, &state, &py("src/report.py"), Some(&player))
        .expect_err("below threshold refuses");
    assert_eq!(err.message(), refusal_line("exports/report.pdf", "stamp.txt"));
}

#[test]
fn the_walk_trusts_an_answer_at_exactly_point_six() {
    let stamp = stamp_of(&format!("{FILES}{ART_HTML}"));
    let state = state_for(&stamp, &[], &[]);
    let stub = StubSource {
        answer: Some(choice_answer(0.6)),
    };
    let entries = recall_walk(&stamp, &state, &py("src/report.py"), Some(&stub))
        .expect("point six walks");
    assert_eq!(entries[0].rev_class, py("reversible"));
    assert!(entries[0].proposed);
    let stub = StubSource {
        answer: Some(choice_answer(0.599999)),
    };
    let err = recall_walk(&stamp, &state, &py("src/report.py"), Some(&stub))
        .expect_err("below point six refuses");
    assert!(matches!(err, Error::Stamp(_)), "{err:?}");
}

#[test]
fn a_malformed_foreign_source_counts_as_no_judgment() {
    let stamp = stamp_of(&format!("{FILES}{ART_HTML}"));
    let state = state_for(&stamp, &[], &[]);
    let bad = obj(vec![
        ("kind", s("choice")),
        ("choice", s("reversible")),
        ("confidence", Value::Float(0.9)),
    ]);
    let stub = StubSource { answer: Some(bad) };
    let err = recall_walk(&stamp, &state, &py("src/report.py"), Some(&stub))
        .expect_err("malformed source refuses");
    assert_eq!(err.message(), refusal_line("build/report.html", "stamp.txt"));
}

#[test]
fn a_classed_artifact_is_never_asked_about() {
    let dir = common::test_dir("judgment-spy");
    let stamp = stamp_of(&format!("{FILES}{ART_PAGE}{ART_HTML}"));
    let page = stamp
        .artifacts
        .iter()
        .find(|a| a.path == py("build/page.html"))
        .expect("page artifact")
        .clone();
    let rigged = obj(vec![
        ("judgment_point", s("classification")),
        ("request", classification_request(&page, "current")),
        ("response", choice_answer(0.95)),
        ("synthetic", Value::Bool(true)),
    ]);
    write_fixture(&dir, "rigged.json", &rigged);
    let committed_text = std::fs::read_to_string(
        root().join("fixtures/classification-report-html-reversible.json"),
    )
    .expect("read committed");
    common::write(&dir, "html.json", &committed_text);
    let player = FixturePlayer::load(dir.as_os_str().as_bytes()).expect("player");
    let spy = SpyPlayer {
        inner: player,
        requests: RefCell::new(Vec::new()),
    };
    let entries = recall_walk(
        &stamp,
        &state_for(&stamp, &[], &[]),
        &py("src/report.py"),
        Some(&spy),
    )
    .expect("walk");
    let page_entry = entries
        .iter()
        .find(|e| e.path == py("build/page.html"))
        .expect("page walked");
    assert_eq!(page_entry.rev_class, py("irreversible"));
    assert!(!page_entry.proposed);
    let asked: Vec<String> = spy
        .requests
        .borrow()
        .iter()
        .map(|r| {
            r.get("artifact")
                .expect("artifact field")
                .as_str()
                .expect("string")
                .to_string()
        })
        .collect();
    assert_eq!(asked, ["build/report.html"]);
}

#[test]
fn an_unreachable_unclassed_artifact_is_never_asked_about() {
    let stamp = stamp_of(&format!("{FILES}{ART_CHART}"));
    let player = FixturePlayer::load(fixtures_dir().as_bytes()).expect("player");
    let spy = SpyPlayer {
        inner: player,
        requests: RefCell::new(Vec::new()),
    };
    let state = state_for(&stamp, &[], &[]);
    let entries = recall_walk(&stamp, &state, &py("src/report.py"), Some(&spy))
        .expect("walk");
    assert!(entries.is_empty());
    assert!(spy.requests.borrow().is_empty());
    let plain = recall_walk(&stamp, &state, &py("src/report.py"), None).expect("walk");
    assert!(plain.is_empty());
}

#[test]
fn a_fully_classed_walk_is_byte_identical_with_and_without_a_source() {
    let stamp = stamp_of(&format!("{FILES}{ART_PAGE}"));
    let state = state_for(&stamp, &[], &[]);
    let player = FixturePlayer::load(fixtures_dir().as_bytes()).expect("player");
    let spy = SpyPlayer {
        inner: player,
        requests: RefCell::new(Vec::new()),
    };
    let plain = recall_walk(&stamp, &state, &py("src/report.py"), None).expect("walk");
    let with_source =
        recall_walk(&stamp, &state, &py("src/report.py"), Some(&spy)).expect("walk");
    assert_eq!(
        format_recall(&py("src/report.py"), &plain),
        format_recall(&py("src/report.py"), &with_source)
    );
    assert!(spy.requests.borrow().is_empty());
}

// The recall command and its fixtures flag, BB-21. These run
// the binary; the stamp and state files stand in a temp dir.

fn cli_stamp(dir: &Path, body: &str) -> String {
    let record = parse_run_record(&py(&format!("{HEADER}{body}")), &py("record.txt"))
        .expect("record");
    let path = common::write(dir, "stamp.txt", &format_stamp(&build_stamp(&record)));
    path.to_str().expect("utf8 path").to_string()
}

fn cli_state(dir: &Path, stamp_path: &str, drifted: &[&str], missing: &[&str]) -> String {
    let text = std::fs::read_to_string(stamp_path).expect("read stamp");
    let stamp = parse_stamp(&py(&text), &py("stamp.txt")).expect("stamp");
    let state = state_for(&stamp, drifted, missing);
    let mut lines = String::new();
    for entry in &stamp.artifacts {
        if let Some(digest) = state.get(&entry.path) {
            lines.push_str(&format!("file {} {}\n", entry.path, digest));
        }
    }
    let path = common::write(dir, "state.txt", &lines);
    path.to_str().expect("utf8 path").to_string()
}

#[test]
fn recall_with_fixtures_prints_the_proposals() {
    let dir = common::test_dir("judgment-cli");
    let stamp = cli_stamp(&dir, &format!("{FILES}{ART_HTML}{ART_SUMMARY}"));
    let state = cli_state(&dir, &stamp, &["docs/summary.md"], &[]);
    let fixtures = fixtures_dir();
    let before = (
        common::read(Path::new(&stamp)),
        common::read(Path::new(&state)),
    );
    let outcome = common::run_cli(
        &["recall", &stamp, &state, "src/report.py", "--fixtures", &fixtures],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    assert_eq!(outcome.stderr_text(), "");
    assert_eq!(
        outcome.stdout_text(),
        "recall src/report.py\n\
artifact build/report.html reversible proposed current chain src/report.py -> build/report.html\n\
artifact docs/summary.md reversible-with-compensation proposed drifted chain src/report.py -> build/report.html -> docs/summary.md\n"
    );
    let after = (
        common::read(Path::new(&stamp)),
        common::read(Path::new(&state)),
    );
    assert_eq!(before, after);
}

#[test]
fn recall_without_fixtures_refuses_with_the_round_1_line() {
    let dir = common::test_dir("judgment-cli-plain");
    let stamp = cli_stamp(&dir, &format!("{FILES}{ART_HTML}"));
    let state = cli_state(&dir, &stamp, &[], &[]);
    let outcome = common::run_cli(&["recall", &stamp, &state, "src/report.py"], &dir);
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
    assert_eq!(
        outcome.stderr_text(),
        format!("error: {}\n", refusal_line("build/report.html", &stamp))
    );
}

#[test]
fn recall_with_a_missing_fixtures_directory_exits_two() {
    let dir = common::test_dir("judgment-cli-ghost");
    let stamp = cli_stamp(&dir, &format!("{FILES}{ART_PAGE}"));
    let state = cli_state(&dir, &stamp, &[], &[]);
    let ghost = dir.join("ghost").to_str().expect("utf8").to_string();
    let outcome = common::run_cli(
        &["recall", &stamp, &state, "src/report.py", "--fixtures", &ghost],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
    assert_eq!(outcome.stderr_text().matches('\n').count(), 1);
    assert!(!outcome.stderr_text().contains("Traceback"));
}

#[test]
fn recall_with_a_malformed_fixture_exits_two() {
    let dir = common::test_dir("judgment-cli-bad");
    let stamp = cli_stamp(&dir, &format!("{FILES}{ART_HTML}"));
    let state = cli_state(&dir, &stamp, &[], &[]);
    let bad_dir = common::test_dir("judgment-cli-badfix");
    common::write(&bad_dir, "bad.json", "this is not json\n");
    let bad = bad_dir.to_str().expect("utf8").to_string();
    let outcome = common::run_cli(
        &["recall", &stamp, &state, "src/report.py", "--fixtures", &bad],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
    assert_eq!(outcome.stderr_text().matches('\n').count(), 1);
    assert!(!outcome.stderr_text().contains("Traceback"));
}

#[test]
fn a_fully_classed_recall_is_byte_identical_with_and_without_the_flag() {
    let dir = common::test_dir("judgment-cli-classed");
    let stamp = cli_stamp(&dir, &format!("{FILES}{ART_PAGE}"));
    let state = cli_state(&dir, &stamp, &[], &[]);
    let fixtures = fixtures_dir();
    let plain = common::run_cli(&["recall", &stamp, &state, "src/report.py"], &dir);
    assert_eq!(plain.code, 0);
    let with_flag = common::run_cli(
        &["recall", &stamp, &state, "src/report.py", "--fixtures", &fixtures],
        &dir,
    );
    assert_eq!(with_flag.code, 0);
    assert_eq!(plain.stdout, with_flag.stdout);
    assert!(!plain.stdout_text().contains("proposed"));
}

// The Round 2 review pins, BB-22 and the BB-21 path order clause.

#[test]
fn two_unproposed_artifacts_refuse_at_the_first_in_path_order() {
    let art_zzz = "artifact out/zzz.html derived-from src/report.py class unclassed\ncontent\nz\nend\n";
    let art_aaa = "artifact out/aaa.html derived-from out/zzz.html class unclassed\ncontent\na\nend\n";
    let stamp = stamp_of(&format!("{FILES}{art_zzz}{art_aaa}"));
    let err = recall_walk(&stamp, &state_for(&stamp, &[], &[]), &py("src/report.py"), None)
        .expect_err("refuses");
    assert_eq!(err.message(), refusal_line("out/aaa.html", "stamp.txt"));
}

#[test]
fn a_non_string_kind_counts_as_no_judgment() {
    let mut answer = choice_answer(0.9);
    set_field(&mut answer, "kind", Value::Array(vec![]));
    assert_eq!(check_answer(Some(answer.clone())), None);
}

#[test]
fn an_unconvertible_number_counts_as_no_judgment() {
    let mut answer = choice_answer(0.9);
    set_field(
        &mut answer,
        "confidence",
        Value::Int(format!("1{}", "0".repeat(400))),
    );
    assert_eq!(check_answer(Some(answer.clone())), None);
}

#[test]
fn a_probability_sum_outside_the_band_counts_as_no_judgment() {
    let mut answer = choice_answer(0.9);
    set_field(
        &mut answer,
        "probabilities",
        obj(vec![
            ("reversible", Value::Float(0.5)),
            ("reversible-with-compensation", Value::Float(0.3)),
            ("irreversible", Value::Float(0.198)),
        ]),
    );
    assert_eq!(check_answer(Some(answer.clone())), None);
}

#[test]
fn a_fixture_with_an_extra_envelope_field_is_refused_at_load() {
    let pair = obj(vec![
        ("synthetic", Value::Bool(true)),
        ("judgment_point", s("classification")),
        ("comment", s("recorded by hand")),
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
    let err = validate_fixture(&pair, "fixture").expect_err("extra field refused");
    assert!(matches!(err, Error::Judgment(_)), "{err:?}");
}
