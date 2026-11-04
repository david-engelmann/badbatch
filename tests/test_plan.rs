//! The plan surface and the ranking point, ported test for
//! test in substance from tests/test_plan.py at the Round 4
//! closure. The Python test for the private request key helper
//! is an in-crate unit test in src/judgment.rs, because the
//! helper is private in the crate exactly as it was in the
//! package.

mod common;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use badbatch::errors::Error;
use badbatch::judgment::{
    FixturePlayer, JudgmentSource, check_ranking_answer, ranking_request, trusted_score,
    validate_fixture,
};
use badbatch::plan::{build_plan, candidates_for, format_plan};
use badbatch::pyjson::Value;
use badbatch::pytext::PyStr;
use badbatch::records::parse_run_record;
use badbatch::stamps::{Stamp, build_stamp, format_stamp, parse_stamp};
use badbatch::walk::recall_walk;

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

fn s(text: &str) -> Value {
    Value::Str(py(text))
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (py(k), v)).collect())
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

const REBUILD: &str = "rebuild-from-parents";
const REMOVE: &str = "remove-artifact";
const QUARANTINE: &str = "quarantine-artifact";

const SESSION: &str = "run run-0031\nagent writer-3\nprompt-hash d15ea5e\nmodel synth-writer-2\n\n\
file src/base.py created\ncontent\nprint(\"base\")\nend\n\n\
artifact out/alpha.html derived-from src/base.py class reversible-with-compensation\ncontent\nalpha\nend\n\n\
artifact out/beta.html derived-from src/base.py class reversible\ncontent\nbeta\nend\n\n\
artifact out/gamma.html derived-from src/base.py class reversible\ncontent\ngamma\nend\n\n\
artifact out/delta.html derived-from src/base.py class reversible-with-compensation\ncontent\ndelta\nend\n";

const PLAIN_PLAN: &str = "plan src/base.py\n\
artifact out/beta.html reversible drifted chain src/base.py -> out/beta.html\n\
compensation quarantine-artifact\n\
compensation remove-artifact\n\
artifact out/gamma.html reversible current chain src/base.py -> out/gamma.html\n\
compensation rebuild-from-parents\n\
compensation remove-artifact\n\
artifact out/alpha.html reversible-with-compensation current chain src/base.py -> out/alpha.html\n\
compensation rebuild-from-parents\n\
compensation remove-artifact\n\
compensation quarantine-artifact\n\
artifact out/delta.html reversible-with-compensation drifted chain src/base.py -> out/delta.html\n\
compensation quarantine-artifact\n\
compensation remove-artifact\n";

const RANKED_PLAN: &str = "plan src/base.py\n\
artifact out/beta.html reversible drifted chain src/base.py -> out/beta.html\n\
compensation quarantine-artifact score 0.8\n\
compensation remove-artifact score 0.8\n\
artifact out/gamma.html reversible current chain src/base.py -> out/gamma.html\n\
compensation rebuild-from-parents\n\
compensation remove-artifact\n\
artifact out/alpha.html reversible-with-compensation current chain src/base.py -> out/alpha.html\n\
compensation remove-artifact score 0.95\n\
compensation quarantine-artifact score 0.7\n\
compensation rebuild-from-parents score 0.62\n\
artifact out/delta.html reversible-with-compensation drifted chain src/base.py -> out/delta.html\n\
compensation quarantine-artifact\n\
compensation remove-artifact\n";

fn session_stamp() -> Stamp {
    let record = parse_run_record(&py(SESSION), &py("record.txt")).expect("record");
    parse_stamp(&py(&format_stamp(&build_stamp(&record))), &py("stamp.txt")).expect("stamp")
}

fn session_state(stamp: &Stamp) -> HashMap<PyStr, PyStr> {
    stamp
        .artifacts
        .iter()
        .map(|entry| {
            let digest = if entry.path == py("out/beta.html") || entry.path == py("out/delta.html")
            {
                py(&"0".repeat(64))
            } else {
                entry.digest.clone()
            };
            (entry.path.clone(), digest)
        })
        .collect()
}

struct CapturingSource {
    classification_requests: RefCell<Vec<Value>>,
    ranking_requests: RefCell<Vec<Value>>,
    classification_answer: Option<Value>,
}

impl CapturingSource {
    fn new(classification_answer: Option<Value>) -> Self {
        CapturingSource {
            classification_requests: RefCell::new(Vec::new()),
            ranking_requests: RefCell::new(Vec::new()),
            classification_answer,
        }
    }
}

impl JudgmentSource for CapturingSource {
    fn judge(&self, request: &Value) -> Option<Value> {
        self.classification_requests
            .borrow_mut()
            .push(request.clone());
        self.classification_answer.clone()
    }
    fn judge_ranking(&self, request: &Value) -> Option<Value> {
        self.ranking_requests.borrow_mut().push(request.clone());
        None
    }
}

struct StubRankingSource {
    answers: HashMap<(String, String), Value>,
    ranking_requests: RefCell<Vec<Value>>,
}

impl StubRankingSource {
    fn new(answers: HashMap<(String, String), Value>) -> Self {
        StubRankingSource {
            answers,
            ranking_requests: RefCell::new(Vec::new()),
        }
    }
}

impl JudgmentSource for StubRankingSource {
    fn judge(&self, _request: &Value) -> Option<Value> {
        None
    }
    fn judge_ranking(&self, request: &Value) -> Option<Value> {
        self.ranking_requests.borrow_mut().push(request.clone());
        let artifact = request
            .get("artifact")
            .and_then(|v| v.as_str())
            .map(|p| p.to_string());
        let candidate = request
            .get("candidate")
            .and_then(|v| v.as_str())
            .map(|p| p.to_string());
        match (artifact, candidate) {
            (Some(a), Some(c)) => self.answers.get(&(a, c)).cloned(),
            _ => None,
        }
    }
}

fn score_answer(score: f64, confidence: f64) -> Value {
    obj(vec![
        ("kind", s("score")),
        ("score", Value::Float(score)),
        ("confidence", Value::Float(confidence)),
    ])
}

// The candidate table, BB-25.

#[test]
fn the_candidate_table_by_class_and_status() {
    let cases: Vec<(&str, &str, Vec<&str>)> = vec![
        ("reversible", "current", vec![REBUILD, REMOVE]),
        (
            "reversible-with-compensation",
            "current",
            vec![REBUILD, REMOVE, QUARANTINE],
        ),
        ("irreversible", "current", vec![QUARANTINE]),
        ("reversible", "drifted", vec![QUARANTINE, REMOVE]),
        (
            "reversible-with-compensation",
            "drifted",
            vec![QUARANTINE, REMOVE],
        ),
        ("irreversible", "drifted", vec![QUARANTINE, REMOVE]),
        ("reversible", "missing", vec![]),
        ("reversible-with-compensation", "missing", vec![]),
        ("irreversible", "missing", vec![]),
        // The BB-25 edge the fix pass added. A class outside the
        // three with a drifted status still yields the drifted
        // pair, because the status checks run first.
        ("unclassed", "drifted", vec![QUARANTINE, REMOVE]),
    ];
    for (class, status, expected) in cases {
        assert_eq!(
            candidates_for(&py(class), &py(status)).expect("candidates"),
            expected,
            "{class} {status}"
        );
    }
}

#[test]
fn candidates_for_an_unknown_class_refuses() {
    let err = candidates_for(&py("unclassed"), &py("current")).expect_err("unknown class refuses");
    assert!(matches!(err, Error::Walk(_)), "{err:?}");
}

// The deterministic plan document, BB-24 and BB-25.

#[test]
fn the_plain_plan_over_the_session_is_exact() {
    let stamp = session_stamp();
    let planned =
        build_plan(&stamp, &session_state(&stamp), &py("src/base.py"), None).expect("plan");
    assert_eq!(format_plan(&py("src/base.py"), &planned), PLAIN_PLAN);
}

#[test]
fn a_missing_artifact_prints_the_none_required_line() {
    let stamp = session_stamp();
    let mut state = session_state(&stamp);
    state.remove(&py("out/gamma.html"));
    let planned = build_plan(&stamp, &state, &py("src/base.py"), None).expect("plan");
    let text = format_plan(&py("src/base.py"), &planned);
    assert!(
        text.contains(
            "artifact out/gamma.html reversible missing chain src/base.py -> out/gamma.html\n\
compensation none required, the artifact is already absent\n"
        ),
        "{text}"
    );
}

#[test]
fn a_plan_with_no_downstream_artifacts_is_the_header_alone() {
    let record = parse_run_record(
        &py("run run-0032\nagent writer-3\nprompt-hash d15ea5e\nmodel synth-writer-2\n\nfile src/lonely.py created\ncontent\nprint(1)\nend\n"),
        &py("record.txt"),
    )
    .expect("record");
    let stamp =
        parse_stamp(&py(&format_stamp(&build_stamp(&record))), &py("stamp.txt")).expect("stamp");
    let planned = build_plan(&stamp, &HashMap::new(), &py("src/lonely.py"), None).expect("plan");
    assert!(planned.is_empty());
    assert_eq!(
        format_plan(&py("src/lonely.py"), &planned),
        "plan src/lonely.py\n"
    );
}

// The ranking point over the committed fixtures, BB-26 to BB-28.

#[test]
fn the_committed_ranking_fixtures_rank_the_session() {
    let stamp = session_stamp();
    let dir = format!("{}/fixtures-ranking", env!("CARGO_MANIFEST_DIR"));
    let player = FixturePlayer::load(dir.as_bytes()).expect("player");
    let planned = build_plan(
        &stamp,
        &session_state(&stamp),
        &py("src/base.py"),
        Some(&player),
    )
    .expect("plan");
    assert_eq!(format_plan(&py("src/base.py"), &planned), RANKED_PLAN);
}

#[test]
fn every_committed_ranking_fixture_matches_the_recorded_shape() {
    let dir = root().join("fixtures-ranking");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 9);
    for name in &names {
        let text = std::fs::read_to_string(dir.join(name)).expect("read fixture");
        let pair = badbatch::pyjson::loads(&py(&text)).expect("parses");
        let validated = validate_fixture(&pair, name).expect("validates");
        assert_eq!(validated, pair);
    }
}

#[test]
fn the_committed_ranking_requests_are_the_builders_requests() {
    let stamp = session_stamp();
    let source = CapturingSource::new(None);
    build_plan(
        &stamp,
        &session_state(&stamp),
        &py("src/base.py"),
        Some(&source),
    )
    .expect("plan");
    let canonical = |value: &Value| {
        let mut text = String::new();
        badbatch::pyjson::dumps_canonical(value, &mut text);
        text
    };
    let built: HashSet<String> = source
        .ranking_requests
        .borrow()
        .iter()
        .map(&canonical)
        .collect();
    let dir = root().join("fixtures-ranking");
    let mut recorded = HashSet::new();
    for entry in std::fs::read_dir(&dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "json") {
            let text = std::fs::read_to_string(&path).expect("read");
            let pair = badbatch::pyjson::loads(&py(&text)).expect("parses");
            recorded.insert(canonical(pair.get("request").expect("request")));
        }
    }
    assert_eq!(recorded, built);
    assert!(source.classification_requests.borrow().is_empty());
}

#[test]
fn the_ranking_request_shape_is_exactly_five_fields() {
    let stamp = session_stamp();
    let entries =
        recall_walk(&stamp, &session_state(&stamp), &py("src/base.py"), None).expect("walk");
    let entry = entries
        .iter()
        .find(|e| e.path == py("out/alpha.html"))
        .expect("alpha walked");
    assert_eq!(
        ranking_request(entry, REMOVE),
        obj(vec![
            ("artifact", s("out/alpha.html")),
            ("candidate", s("remove-artifact")),
            ("class", s("reversible-with-compensation")),
            ("status", s("current")),
            ("chain", s("src/base.py -> out/alpha.html")),
        ])
    );
}

#[test]
fn no_ranking_request_for_artifacts_with_fewer_than_two_candidates() {
    let examples = root().join("examples");
    let stamp_text = std::fs::read_to_string(examples.join("stamp.txt")).expect("read stamp");
    let stamp = parse_stamp(&py(&stamp_text), &py("examples/stamp.txt")).expect("stamp");
    let state_text =
        std::fs::read_to_string(examples.join("current-state.txt")).expect("read state");
    let mut state = HashMap::new();
    for line in state_text.split('\n') {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 3 {
            state.insert(py(parts[1]), py(parts[2]));
        }
    }
    let source = CapturingSource::new(None);
    let planned = build_plan(&stamp, &state, &py("src/report.py"), Some(&source)).expect("plan");
    let asked: HashSet<(String, String)> = source
        .ranking_requests
        .borrow()
        .iter()
        .map(|r| {
            (
                r.get("artifact")
                    .and_then(|v| v.as_str())
                    .expect("artifact")
                    .to_string(),
                r.get("candidate")
                    .and_then(|v| v.as_str())
                    .expect("candidate")
                    .to_string(),
            )
        })
        .collect();
    assert_eq!(
        asked,
        HashSet::from([
            ("build/report.html".to_string(), REBUILD.to_string()),
            ("build/report.html".to_string(), REMOVE.to_string()),
        ])
    );
    let text = format_plan(&py("src/report.py"), &planned);
    assert!(text.contains("compensation none required, the artifact is already absent"));
    assert!(text.contains("artifact exports/report.pdf irreversible current"));
}

#[test]
fn a_plan_whose_source_abstains_everywhere_is_byte_identical() {
    let stamp = session_stamp();
    let state = session_state(&stamp);
    let plain = format_plan(
        &py("src/base.py"),
        &build_plan(&stamp, &state, &py("src/base.py"), None).expect("plan"),
    );
    let abstaining = StubRankingSource::new(HashMap::new());
    let with_source = format_plan(
        &py("src/base.py"),
        &build_plan(&stamp, &state, &py("src/base.py"), Some(&abstaining)).expect("plan"),
    );
    assert_eq!(with_source, plain);
    assert_eq!(plain, PLAIN_PLAN);
    assert_eq!(abstaining.ranking_requests.borrow().len(), 9);
}

// The ordering rule, BB-28.

fn plan_text_with(source: &StubRankingSource) -> String {
    let stamp = session_stamp();
    format_plan(
        &py("src/base.py"),
        &build_plan(
            &stamp,
            &session_state(&stamp),
            &py("src/base.py"),
            Some(source),
        )
        .expect("plan"),
    )
}

#[test]
fn a_tie_keeps_canonical_order_and_prints_both_scores() {
    let source = StubRankingSource::new(HashMap::from([
        (
            ("out/gamma.html".to_string(), REBUILD.to_string()),
            score_answer(0.5, 0.9),
        ),
        (
            ("out/gamma.html".to_string(), REMOVE.to_string()),
            score_answer(0.5, 0.9),
        ),
    ]));
    let text = plan_text_with(&source);
    assert!(
        text.contains(
            "compensation rebuild-from-parents score 0.5\ncompensation remove-artifact score 0.5\n"
        ),
        "{text}"
    );
}

#[test]
fn a_scored_candidate_prints_before_an_unscored_one() {
    let source = StubRankingSource::new(HashMap::from([(
        ("out/gamma.html".to_string(), REMOVE.to_string()),
        score_answer(0.7, 0.9),
    )]));
    let text = plan_text_with(&source);
    assert!(
        text.contains(
            "compensation remove-artifact score 0.7\ncompensation rebuild-from-parents\n"
        ),
        "{text}"
    );
}

#[test]
fn the_ranking_trust_edge_is_exactly_point_six() {
    let at = StubRankingSource::new(HashMap::from([(
        ("out/gamma.html".to_string(), REMOVE.to_string()),
        score_answer(0.7, 0.6),
    )]));
    assert!(plan_text_with(&at).contains("compensation remove-artifact score 0.7\n"));
    let below = StubRankingSource::new(HashMap::from([(
        ("out/gamma.html".to_string(), REMOVE.to_string()),
        score_answer(0.7, 0.599999),
    )]));
    assert!(!plan_text_with(&below).contains("score"));
}

#[test]
fn trusted_score_and_check_ranking_answer_units() {
    let good = score_answer(0.7, 0.9);
    assert_eq!(check_ranking_answer(Some(good.clone())), Some(good.clone()));
    assert!(trusted_score(Some(&good)));
    let abstention = obj(vec![("kind", s("abstention"))]);
    assert_eq!(
        check_ranking_answer(Some(abstention.clone())),
        Some(abstention.clone())
    );
    assert!(!trusted_score(Some(&abstention)));
    assert!(!trusted_score(Some(&score_answer(0.7, 0.45))));
    let no_confidence = obj(vec![("kind", s("score")), ("score", Value::Float(0.7))]);
    assert!(!trusted_score(Some(&no_confidence)));
}

#[test]
fn a_malformed_ranking_answer_counts_as_no_judgment() {
    let mut extra = score_answer(0.9, 0.9);
    if let Value::Object(pairs) = &mut extra {
        pairs.push((py("note"), s("extra")));
    }
    let answers: Vec<Value> = vec![
        obj(vec![
            ("kind", s("choice")),
            ("choice", s("reversible")),
            ("confidence", Value::Float(0.9)),
            (
                "probabilities",
                obj(vec![
                    ("reversible", Value::Float(0.9)),
                    ("reversible-with-compensation", Value::Float(0.05)),
                    ("irreversible", Value::Float(0.05)),
                ]),
            ),
        ]),
        extra,
        obj(vec![
            ("kind", s("score")),
            ("score", Value::Int(format!("1{}", "0".repeat(400)))),
            ("confidence", Value::Float(0.9)),
        ]),
        obj(vec![
            ("kind", s("score")),
            ("confidence", Value::Float(0.9)),
        ]),
        obj(vec![
            ("kind", s("abstention")),
            ("score", Value::Float(0.9)),
        ]),
        s("not an answer at all"),
    ];
    for answer in &answers {
        assert_eq!(check_ranking_answer(Some(answer.clone())), None);
        let mut table = HashMap::new();
        for artifact in [
            "out/alpha.html",
            "out/beta.html",
            "out/gamma.html",
            "out/delta.html",
        ] {
            for candidate in [REBUILD, REMOVE, QUARANTINE] {
                table.insert(
                    (artifact.to_string(), candidate.to_string()),
                    answer.clone(),
                );
            }
        }
        let source = StubRankingSource::new(table);
        assert_eq!(plan_text_with(&source), PLAIN_PLAN);
    }
}

#[test]
fn a_ranking_answer_with_extra_keys_is_no_judgment() {
    // The Python original used a dict with an integer key beside
    // a string key. The crate's value model keys objects by
    // strings only, so the mixed type crash class BB-23 closed
    // cannot be constructed here; extra string keys exercise
    // the same refusal.
    let mut answer = score_answer(0.9, 0.9);
    if let Value::Object(pairs) = &mut answer {
        pairs.push((py("7"), s("a numeric string key")));
        pairs.push((py("note"), s("a string key")));
    }
    assert_eq!(check_ranking_answer(Some(answer.clone())), None);
}

// The ranking fixture shape, BB-27.

fn good_ranking_pair() -> Value {
    obj(vec![
        ("judgment_point", s("compensation-ranking")),
        (
            "request",
            obj(vec![
                ("artifact", s("out/alpha.html")),
                ("candidate", s("remove-artifact")),
                ("class", s("reversible-with-compensation")),
                ("status", s("current")),
                ("chain", s("src/base.py -> out/alpha.html")),
            ]),
        ),
        ("response", score_answer(0.95, 0.9)),
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
    common::write_bytes(dir, name, text.as_bytes());
}

fn ranking_mutations() -> Vec<Value> {
    let mut out = Vec::new();
    let mut pair = good_ranking_pair();
    set_field(&mut pair, "synthetic", Value::Bool(false));
    out.push(pair);
    let mut pair = good_ranking_pair();
    set_field(&mut pair, "judgment_point", s("classification"));
    out.push(pair);
    let mut pair = good_ranking_pair();
    set_field(&mut pair, "comment", s("extra envelope field"));
    out.push(pair);
    let mut pair = good_ranking_pair();
    remove_field(field_mut(&mut pair, "response"), "confidence");
    out.push(pair);
    let mut pair = good_ranking_pair();
    set_field(field_mut(&mut pair, "response"), "score", Value::Float(1.5));
    out.push(pair);
    let mut pair = good_ranking_pair();
    set_field(field_mut(&mut pair, "response"), "kind", s("choice"));
    out.push(pair);
    let mut pair = good_ranking_pair();
    remove_field(field_mut(&mut pair, "request"), "chain");
    out.push(pair);
    let mut pair = good_ranking_pair();
    set_field(field_mut(&mut pair, "request"), "extra", s("field"));
    out.push(pair);
    let mut pair = good_ranking_pair();
    set_field(
        field_mut(&mut pair, "request"),
        "candidate",
        s("delete-artifact"),
    );
    out.push(pair);
    let mut pair = good_ranking_pair();
    set_field(field_mut(&mut pair, "request"), "class", s("sideways"));
    out.push(pair);
    out
}

#[test]
fn a_ranking_fixture_outside_the_shape_is_refused_at_load() {
    for (index, pair) in ranking_mutations().iter().enumerate() {
        let dir = common::test_dir("plan-mutation");
        write_fixture(&dir, "bad.json", pair);
        let err = FixturePlayer::load(dir.as_os_str().as_bytes()).expect_err("mutation refused");
        assert!(matches!(err, Error::Judgment(_)), "case {index}: {err:?}");
    }
}

#[test]
fn two_ranking_fixtures_for_one_request_are_refused_at_load() {
    let dir = common::test_dir("plan-duplicate");
    write_fixture(&dir, "one.json", &good_ranking_pair());
    write_fixture(&dir, "two.json", &good_ranking_pair());
    let err = FixturePlayer::load(dir.as_os_str().as_bytes()).expect_err("duplicate refused");
    assert!(matches!(err, Error::Judgment(_)), "{err:?}");
}

#[test]
fn one_directory_answers_each_point_only_from_its_own_pairs() {
    let dir = common::test_dir("plan-mixed");
    for sub in ["fixtures", "fixtures-ranking"] {
        let source = root().join(sub);
        for entry in std::fs::read_dir(&source).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|e| e == "json") {
                let text = std::fs::read_to_string(&path).expect("read");
                common::write(
                    &dir,
                    path.file_name().expect("name").to_str().expect("utf8"),
                    &text,
                );
            }
        }
    }
    let player = FixturePlayer::load(dir.as_os_str().as_bytes()).expect("player");
    let classification_text =
        std::fs::read_to_string(root().join("fixtures/classification-report-html-reversible.json"))
            .expect("read");
    let classification = badbatch::pyjson::loads(&py(&classification_text)).expect("parses");
    let c_request = classification.get("request").expect("request").clone();
    let c_response = classification.get("response").expect("response").clone();
    assert_eq!(player.judge(&c_request), Some(c_response));
    assert_eq!(player.judge_ranking(&c_request), None);
    let ranking_text =
        std::fs::read_to_string(root().join("fixtures-ranking/ranking-alpha-remove-artifact.json"))
            .expect("read");
    let ranking = badbatch::pyjson::loads(&py(&ranking_text)).expect("parses");
    let r_request = ranking.get("request").expect("request").clone();
    let r_response = ranking.get("response").expect("response").clone();
    assert_eq!(player.judge_ranking(&r_request), Some(r_response));
    assert_eq!(player.judge(&r_request), None);
}

// Both points inside one plan, BB-29 and the proposed class rule.

#[test]
fn a_proposed_class_plans_under_its_proposed_class() {
    let dir = common::test_dir("plan-proposed");
    let body = "file src/base.py created\ncontent\nprint(\"base\")\nend\n\
artifact out/page.html derived-from src/base.py class unclassed\ncontent\npage\nend\n\
artifact out/tool.html derived-from src/base.py class reversible\ncontent\ntool\nend\n";
    let record = parse_run_record(
        &py(&format!(
            "run run-0033\nagent writer-3\nprompt-hash d15ea5e\nmodel synth-writer-2\n\n{body}"
        )),
        &py("record.txt"),
    )
    .expect("record");
    let stamp =
        parse_stamp(&py(&format_stamp(&build_stamp(&record))), &py("stamp.txt")).expect("stamp");
    let state: HashMap<PyStr, PyStr> = stamp
        .artifacts
        .iter()
        .map(|e| (e.path.clone(), e.digest.clone()))
        .collect();
    let choice = obj(vec![
        ("kind", s("choice")),
        ("choice", s("reversible")),
        ("confidence", Value::Float(0.9)),
        (
            "probabilities",
            obj(vec![
                ("reversible", Value::Float(0.9)),
                ("reversible-with-compensation", Value::Float(0.05)),
                ("irreversible", Value::Float(0.05)),
            ]),
        ),
    ]);
    let capture = CapturingSource::new(Some(choice.clone()));
    build_plan(&stamp, &state, &py("src/base.py"), Some(&capture)).expect("plan");
    assert_eq!(capture.classification_requests.borrow().len(), 1);
    let classification_pair = obj(vec![
        ("synthetic", Value::Bool(true)),
        ("judgment_point", s("classification")),
        (
            "request",
            capture.classification_requests.borrow()[0].clone(),
        ),
        ("response", choice.clone()),
    ]);
    write_fixture(&dir, "classification.json", &classification_pair);
    for request in capture.ranking_requests.borrow().iter() {
        let artifact = request
            .get("artifact")
            .and_then(|v| v.as_str())
            .expect("artifact")
            .to_string();
        let candidate = request
            .get("candidate")
            .and_then(|v| v.as_str())
            .expect("candidate")
            .to_string();
        let response = if artifact == "out/page.html" {
            score_answer(if candidate == REMOVE { 0.9 } else { 0.7 }, 0.9)
        } else {
            obj(vec![("kind", s("abstention"))])
        };
        let pair = obj(vec![
            ("synthetic", Value::Bool(true)),
            ("judgment_point", s("compensation-ranking")),
            ("request", request.clone()),
            ("response", response),
        ]);
        let stem = artifact
            .rsplit('/')
            .next()
            .expect("stem")
            .split('.')
            .next()
            .expect("stem");
        write_fixture(&dir, &format!("ranking-{stem}-{candidate}.json"), &pair);
    }
    let player = FixturePlayer::load(dir.as_os_str().as_bytes()).expect("player");
    let planned = build_plan(&stamp, &state, &py("src/base.py"), Some(&player)).expect("plan");
    assert_eq!(
        format_plan(&py("src/base.py"), &planned),
        "plan src/base.py\n\
artifact out/page.html reversible proposed current chain src/base.py -> out/page.html\n\
compensation remove-artifact score 0.9\n\
compensation rebuild-from-parents score 0.7\n\
artifact out/tool.html reversible current chain src/base.py -> out/tool.html\n\
compensation rebuild-from-parents\n\
compensation remove-artifact\n"
    );
}

#[test]
fn the_recall_is_unchanged_by_ranking_pairs_in_its_directory() {
    let dir = common::test_dir("plan-recall-mixed");
    let mixed = dir.join("mixed");
    std::fs::create_dir_all(&mixed).expect("mixed dir");
    for sub in ["fixtures", "fixtures-ranking"] {
        let source = root().join(sub);
        for entry in std::fs::read_dir(&source).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|e| e == "json") {
                let text = std::fs::read_to_string(&path).expect("read");
                common::write(
                    &mixed,
                    path.file_name().expect("name").to_str().expect("utf8"),
                    &text,
                );
            }
        }
    }
    let body = "file src/report.py created\ncontent\nprint(\"report\")\nend\n\
file src/data.csv created\ncontent\na,b\nend\n\
artifact build/report.html derived-from src/report.py class unclassed\ncontent\n<html>report</html>\nend\n\
artifact docs/summary.md derived-from build/report.html class unclassed\ncontent\nsummary\nend\n";
    let record = parse_run_record(
        &py(&format!(
            "run run-0021\nagent writer-3\nprompt-hash c07ffee\nmodel synth-writer-2\n\n{body}"
        )),
        &py("record.txt"),
    )
    .expect("record");
    let stamp_path = common::write(&dir, "stamp.txt", &format_stamp(&build_stamp(&record)));
    let stamp = parse_stamp(
        &py(&std::fs::read_to_string(&stamp_path).expect("read stamp")),
        &py("stamp.txt"),
    )
    .expect("stamp");
    let mut lines = String::new();
    for entry in &stamp.artifacts {
        let digest = if entry.path == py("docs/summary.md") {
            "0".repeat(64)
        } else {
            entry.digest.to_string()
        };
        lines.push_str(&format!("file {} {}\n", entry.path, digest));
    }
    let state_path = common::write(&dir, "state.txt", &lines);
    let stamp_arg = stamp_path.to_str().expect("utf8").to_string();
    let state_arg = state_path.to_str().expect("utf8").to_string();
    let fixtures = format!("{}/fixtures", env!("CARGO_MANIFEST_DIR"));
    let plain = common::run_cli(
        &[
            "recall",
            &stamp_arg,
            &state_arg,
            "src/report.py",
            "--fixtures",
            &fixtures,
        ],
        &dir,
    );
    assert_eq!(plain.code, 0);
    let mixed_arg = mixed.to_str().expect("utf8").to_string();
    let with_ranking = common::run_cli(
        &[
            "recall",
            &stamp_arg,
            &state_arg,
            "src/report.py",
            "--fixtures",
            &mixed_arg,
        ],
        &dir,
    );
    assert_eq!(with_ranking.code, 0);
    assert_eq!(with_ranking.stdout, plain.stdout);
    assert!(with_ranking.stdout_text().contains("proposed"));
}

// The plan command, BB-24 and BB-29.

#[test]
fn the_plan_command_prints_the_plan_and_exits_zero() {
    let dir = common::test_dir("plan-cli");
    let examples = root().join("examples");
    let stamp = common::write(
        &dir,
        "stamp.txt",
        &std::fs::read_to_string(examples.join("stamp.txt")).expect("read"),
    );
    let state = common::write(
        &dir,
        "state.txt",
        &std::fs::read_to_string(examples.join("current-state.txt")).expect("read"),
    );
    let stamp_arg = stamp.to_str().expect("utf8").to_string();
    let state_arg = state.to_str().expect("utf8").to_string();
    let before = (common::read(&stamp), common::read(&state));
    let outcome = common::run_cli(&["plan", &stamp_arg, &state_arg, "src/report.py"], &dir);
    assert_eq!(outcome.code, 0);
    assert_eq!(outcome.stderr_text(), "");
    assert_eq!(
        outcome.stdout_text(),
        "plan src/report.py\n\
artifact build/report.html reversible current chain src/report.py -> build/report.html\n\
compensation rebuild-from-parents\n\
compensation remove-artifact\n\
artifact docs/summary.md reversible-with-compensation missing chain src/report.py -> build/report.html -> docs/summary.md\n\
compensation none required, the artifact is already absent\n\
artifact exports/report.pdf irreversible current chain src/report.py -> build/report.html -> docs/summary.md -> exports/report.pdf\n\
compensation quarantine-artifact\n"
    );
    let after = (common::read(&stamp), common::read(&state));
    assert_eq!(before, after);
}

#[test]
fn the_plan_command_ranks_with_the_committed_fixtures() {
    let dir = common::test_dir("plan-cli-ranked");
    let record = parse_run_record(&py(SESSION), &py("record.txt")).expect("record");
    let stamp = common::write(&dir, "stamp.txt", &format_stamp(&build_stamp(&record)));
    let parsed = parse_stamp(
        &py(&std::fs::read_to_string(&stamp).expect("read")),
        &py("stamp.txt"),
    )
    .expect("stamp");
    let state_map = session_state(&parsed);
    let mut lines = String::new();
    for entry in &parsed.artifacts {
        lines.push_str(&format!(
            "file {} {}\n",
            entry.path,
            state_map.get(&entry.path).expect("state")
        ));
    }
    let state = common::write(&dir, "state.txt", &lines);
    let ranking = format!("{}/fixtures-ranking", env!("CARGO_MANIFEST_DIR"));
    let outcome = common::run_cli(
        &[
            "plan",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "src/base.py",
            "--fixtures",
            &ranking,
        ],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    assert_eq!(outcome.stderr_text(), "");
    assert_eq!(outcome.stdout_text(), RANKED_PLAN);
}

#[test]
fn the_plan_command_refuses_an_unproposed_unclassed_artifact() {
    let dir = common::test_dir("plan-cli-refusal");
    let body = "file src/base.py created\ncontent\nprint(\"base\")\nend\n\
artifact out/page.html derived-from src/base.py class unclassed\ncontent\npage\nend\n";
    let record = parse_run_record(
        &py(&format!(
            "run run-0033\nagent writer-3\nprompt-hash d15ea5e\nmodel synth-writer-2\n\n{body}"
        )),
        &py("record.txt"),
    )
    .expect("record");
    let stamp = common::write(&dir, "stamp.txt", &format_stamp(&build_stamp(&record)));
    let parsed = parse_stamp(
        &py(&std::fs::read_to_string(&stamp).expect("read")),
        &py("stamp.txt"),
    )
    .expect("stamp");
    let mut lines = String::new();
    for entry in &parsed.artifacts {
        lines.push_str(&format!("file {} {}\n", entry.path, entry.digest));
    }
    let state = common::write(&dir, "state.txt", &lines);
    let stamp_arg = stamp.to_str().expect("utf8").to_string();
    let outcome = common::run_cli(
        &[
            "plan",
            &stamp_arg,
            state.to_str().expect("utf8"),
            "src/base.py",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
    assert_eq!(
        outcome.stderr_text(),
        format!(
            "error: {stamp_arg} is malformed, the artifact line for out/page.html \
             names the class 'unclassed', which is not one of the three named classes\n"
        )
    );
}

#[test]
fn the_plan_command_with_a_missing_fixtures_directory_exits_two() {
    let dir = common::test_dir("plan-cli-ghost");
    let examples = root().join("examples");
    let stamp = common::write(
        &dir,
        "stamp.txt",
        &std::fs::read_to_string(examples.join("stamp.txt")).expect("read"),
    );
    let state = common::write(
        &dir,
        "state.txt",
        &std::fs::read_to_string(examples.join("current-state.txt")).expect("read"),
    );
    let ghost = dir.join("ghost").to_str().expect("utf8").to_string();
    let outcome = common::run_cli(
        &[
            "plan",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "src/report.py",
            "--fixtures",
            &ghost,
        ],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
    assert_eq!(outcome.stderr_text().matches('\n').count(), 1);
    assert!(!outcome.stderr_text().contains("Traceback"));
}

#[test]
fn a_non_string_judgment_point_refuses_at_load() {
    for point in ["[\"classification\"]", "{\"point\": \"classification\"}"] {
        let dir = common::test_dir("plan-point");
        // Every other part of the pair is valid in full, so no
        // other refusal in the loader can fire first or
        // instead. The refusal asserted is the point check's
        // own message. A loader that let a non-string point
        // fall through to the request and response checks
        // would load this pair, and this test would fail.
        let text = format!(
            "{{\"synthetic\": true, \"judgment_point\": {point}, \
             \"request\": {{\"artifact\": \"build/report.html\", \
             \"parents\": [\"src/report.py\"], \"digest\": \"{}\", \
             \"status\": \"current\"}}, \
             \"response\": {{\"kind\": \"choice\", \"choice\": \"reversible\", \
             \"confidence\": 0.9, \"probabilities\": {{\"reversible\": 0.9, \
             \"reversible-with-compensation\": 0.05, \"irreversible\": 0.05}}}}}}",
            "a".repeat(64)
        );
        common::write(&dir, "bad.json", &text);
        let err =
            FixturePlayer::load(dir.as_os_str().as_bytes()).expect_err("non-string point refused");
        assert!(matches!(err, Error::Judgment(_)), "{err:?}");
        assert_eq!(
            err.message(),
            "fixture bad.json field 'judgment_point' names no judgment \
             point this tool records"
        );
    }
}

fn alpha_compensation_lines(scores: &[(&str, f64)]) -> Vec<String> {
    let stamp = session_stamp();
    let answers: HashMap<(String, String), Value> = scores
        .iter()
        .map(|(candidate, value)| {
            (
                ("out/alpha.html".to_string(), candidate.to_string()),
                score_answer(*value, 0.9),
            )
        })
        .collect();
    let source = StubRankingSource::new(answers);
    let entries = build_plan(
        &stamp,
        &session_state(&stamp),
        &py("src/base.py"),
        Some(&source),
    )
    .expect("plan");
    let alpha: Vec<_> = entries
        .into_iter()
        .filter(|e| e.path == py("out/alpha.html"))
        .collect();
    let text = format_plan(&py("src/a.py"), &alpha);
    text.lines()
        .filter(|l| l.starts_with("compensation"))
        .map(|l| l.to_string())
        .collect()
}

#[test]
fn three_candidates_mixed_scored_and_unscored_order() {
    let lines = alpha_compensation_lines(&[(REBUILD, 0.62), (QUARANTINE, 0.95)]);
    assert_eq!(
        lines,
        [
            "compensation quarantine-artifact score 0.95",
            "compensation rebuild-from-parents score 0.62",
            "compensation remove-artifact",
        ]
    );
}

#[test]
fn three_candidates_all_tied_keep_canonical_order() {
    let lines = alpha_compensation_lines(&[(REBUILD, 0.8), (REMOVE, 0.8), (QUARANTINE, 0.8)]);
    let names: Vec<String> = lines
        .iter()
        .map(|l| l.split_whitespace().nth(1).expect("name").to_string())
        .collect();
    assert_eq!(names, [REBUILD, REMOVE, QUARANTINE]);
}

#[test]
fn a_negative_zero_score_prints_as_zero() {
    let lines = alpha_compensation_lines(&[(REBUILD, -0.0)]);
    let joined = lines.join("\n");
    assert!(joined.contains("score 0.0"), "{joined}");
    assert!(!joined.contains("-0.0"), "{joined}");
}
