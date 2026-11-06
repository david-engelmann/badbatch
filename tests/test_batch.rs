//! The batch walk, Round 6. The recall walk and the
//! execution plan start from several stamped files at once,
//! under BB-42 to BB-47. The pins here are the multi-start
//! forms. The single-start forms stand pinned by the rest of
//! the suite, the committed corpus among them, and the batch
//! walk must leave every one of them byte-identical.

mod common;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use badbatch::errors::Error;
use badbatch::judgment::JudgmentSource;
use badbatch::plan::{build_plan_multi, format_plan_multi};
use badbatch::pyjson::Value;
use badbatch::pytext::PyStr;
use badbatch::stamps::{Stamp, parse_stamp};
use badbatch::walk::{format_recall_multi, recall_walk_multi};

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

fn d(c: char) -> String {
    c.to_string().repeat(64)
}

fn s(text: &str) -> Value {
    Value::Str(py(text))
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (py(k), v)).collect())
}

/// The batch session. Three files head three branches that
/// cross at two artifacts. Out/joint.txt stands at the end of
/// a long road from gen/a.txt, through mid/a1.txt and
/// mid/a2.txt, and at the end of a short road from gen/b.txt,
/// which is the shape the chain rule of BB-43 decides.
fn batch_stamp() -> Stamp {
    let text = format!(
        "stamp run-0042\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file gen/a.txt created {a}\n\
file gen/b.txt created {b}\n\
file gen/c.txt created {c}\n\
artifact mid/only-a.txt reversible {oa} derived-from gen/a.txt\n\
artifact mid/only-b.txt reversible {ob} derived-from gen/b.txt\n\
artifact mid/only-c.txt reversible-with-compensation {oc} derived-from gen/c.txt\n\
artifact mid/shared.txt reversible {sh} derived-from gen/a.txt, gen/b.txt\n\
artifact mid/a1.txt reversible {a1} derived-from gen/a.txt\n\
artifact mid/a2.txt reversible {a2} derived-from mid/a1.txt\n\
artifact out/joint.txt irreversible {jo} derived-from mid/a2.txt, gen/b.txt\n\
artifact out/deep.txt reversible-with-compensation {de} derived-from mid/shared.txt\n\
artifact out/beyond.txt reversible {be} derived-from out/joint.txt\n",
        a = d('1'),
        b = d('2'),
        c = d('3'),
        oa = d('a'),
        ob = d('b'),
        oc = d('c'),
        sh = d('d'),
        a1 = d('e'),
        a2 = d('f'),
        jo = d('4'),
        de = d('5'),
        be = d('6'),
    );
    parse_stamp(&py(&text), &py("stamp.txt")).expect("batch stamp")
}

fn batch_state() -> HashMap<PyStr, PyStr> {
    [
        ("mid/only-a.txt", 'a'),
        ("mid/only-b.txt", 'b'),
        ("mid/only-c.txt", 'c'),
        ("mid/shared.txt", 'd'),
        ("mid/a1.txt", 'e'),
        ("mid/a2.txt", 'f'),
        ("out/joint.txt", '4'),
        ("out/deep.txt", '5'),
        ("out/beyond.txt", '6'),
    ]
    .iter()
    .map(|(path, c)| (py(path), py(&d(*c))))
    .collect()
}

fn starts(names: &[&str]) -> Vec<PyStr> {
    names.iter().map(|name| py(name)).collect()
}

fn chain_of(entries: &[badbatch::walk::WalkEntry], path: &str) -> Vec<PyStr> {
    entries
        .iter()
        .find(|entry| entry.path == py(path))
        .unwrap_or_else(|| panic!("{path} walked"))
        .chain
        .clone()
}

/// One judgment source that records both points and answers
/// under a chain gate. The classification point answers the
/// held answer, or none. The ranking point answers a trusted
/// score only for the one artifact and chain the gate names,
/// which is the shape BB-47 turns on.
struct SpySource {
    classification_requests: RefCell<Vec<Value>>,
    ranking_requests: RefCell<Vec<Value>>,
    classification_answer: Option<Value>,
    ranking_gate: Option<(String, String)>,
}

impl SpySource {
    fn new() -> Self {
        SpySource {
            classification_requests: RefCell::new(Vec::new()),
            ranking_requests: RefCell::new(Vec::new()),
            classification_answer: None,
            ranking_gate: None,
        }
    }
}

impl JudgmentSource for SpySource {
    fn judge(&self, request: &Value) -> Option<Value> {
        self.classification_requests
            .borrow_mut()
            .push(request.clone());
        self.classification_answer.clone()
    }
    fn judge_ranking(&self, request: &Value) -> Option<Value> {
        self.ranking_requests.borrow_mut().push(request.clone());
        let (gate_artifact, gate_chain) = self.ranking_gate.as_ref()?;
        let artifact = request.get("artifact").and_then(|held| held.as_str());
        let chain = request.get("chain").and_then(|held| held.as_str());
        if artifact.map(|held| held.to_string()).as_ref() == Some(gate_artifact)
            && chain.map(|held| held.to_string()).as_ref() == Some(gate_chain)
        {
            Some(obj(vec![
                ("kind", s("score")),
                ("score", Value::Float(0.9)),
                ("confidence", Value::Float(0.9)),
            ]))
        } else {
            None
        }
    }
}

// The union and the chains, BB-42 and BB-43.

#[test]
fn two_starts_list_the_union_of_their_walks_once_each() {
    let stamp = batch_stamp();
    let entries = recall_walk_multi(
        &stamp,
        &batch_state(),
        &starts(&["gen/a.txt", "gen/b.txt"]),
        None,
    )
    .expect("batch walk");
    let paths: Vec<String> = entries.iter().map(|entry| entry.path.to_string()).collect();
    assert_eq!(
        paths,
        [
            "mid/a1.txt",
            "mid/a2.txt",
            "mid/only-a.txt",
            "mid/only-b.txt",
            "mid/shared.txt",
            "out/beyond.txt",
            "out/deep.txt",
            "out/joint.txt",
        ]
    );
    // The artifact only the third file reaches stays out.
    assert!(!paths.iter().any(|path| path == "mid/only-c.txt"));
}

#[test]
fn disjoint_starts_carry_their_own_chains() {
    let stamp = batch_stamp();
    let entries = recall_walk_multi(
        &stamp,
        &batch_state(),
        &starts(&["gen/a.txt", "gen/c.txt"]),
        None,
    )
    .expect("batch walk");
    assert_eq!(
        chain_of(&entries, "mid/only-c.txt"),
        vec![py("gen/c.txt"), py("mid/only-c.txt")]
    );
    assert_eq!(
        chain_of(&entries, "out/deep.txt"),
        vec![py("gen/a.txt"), py("mid/shared.txt"), py("out/deep.txt")]
    );
}

#[test]
fn the_earliest_start_owns_the_chain_even_when_its_road_is_longer() {
    let stamp = batch_stamp();
    let entries = recall_walk_multi(
        &stamp,
        &batch_state(),
        &starts(&["gen/a.txt", "gen/b.txt"]),
        None,
    )
    .expect("batch walk");
    // Gen/b.txt reaches out/joint.txt in one step. The walk
    // still prints the long road from gen/a.txt, the earliest
    // start, and the road onward to out/beyond.txt runs
    // through it on the same terms.
    assert_eq!(
        chain_of(&entries, "out/joint.txt"),
        vec![
            py("gen/a.txt"),
            py("mid/a1.txt"),
            py("mid/a2.txt"),
            py("out/joint.txt")
        ]
    );
    assert_eq!(
        chain_of(&entries, "out/beyond.txt"),
        vec![
            py("gen/a.txt"),
            py("mid/a1.txt"),
            py("mid/a2.txt"),
            py("out/joint.txt"),
            py("out/beyond.txt")
        ]
    );
    assert_eq!(
        chain_of(&entries, "mid/shared.txt"),
        vec![py("gen/a.txt"), py("mid/shared.txt")]
    );
}

#[test]
fn reversing_the_starts_reverses_the_chain_ownership() {
    let stamp = batch_stamp();
    let entries = recall_walk_multi(
        &stamp,
        &batch_state(),
        &starts(&["gen/b.txt", "gen/a.txt"]),
        None,
    )
    .expect("batch walk");
    assert_eq!(
        chain_of(&entries, "out/joint.txt"),
        vec![py("gen/b.txt"), py("out/joint.txt")]
    );
    assert_eq!(
        chain_of(&entries, "out/beyond.txt"),
        vec![py("gen/b.txt"), py("out/joint.txt"), py("out/beyond.txt")]
    );
    assert_eq!(
        chain_of(&entries, "mid/shared.txt"),
        vec![py("gen/b.txt"), py("mid/shared.txt")]
    );
    // What only the later start reaches keeps its own chain.
    assert_eq!(
        chain_of(&entries, "mid/only-a.txt"),
        vec![py("gen/a.txt"), py("mid/only-a.txt")]
    );
}

// The header lines, BB-44.

#[test]
fn the_document_opens_with_one_header_line_per_start_in_argument_order() {
    let stamp = batch_stamp();
    let entries = recall_walk_multi(
        &stamp,
        &batch_state(),
        &starts(&["gen/b.txt", "gen/a.txt"]),
        None,
    )
    .expect("batch walk");
    let text = format_recall_multi(&starts(&["gen/b.txt", "gen/a.txt"]), &entries);
    assert_eq!(
        text,
        "recall gen/b.txt\n\
recall gen/a.txt\n\
artifact mid/a1.txt reversible current chain gen/a.txt -> mid/a1.txt\n\
artifact mid/a2.txt reversible current chain gen/a.txt -> mid/a1.txt -> mid/a2.txt\n\
artifact mid/only-a.txt reversible current chain gen/a.txt -> mid/only-a.txt\n\
artifact mid/only-b.txt reversible current chain gen/b.txt -> mid/only-b.txt\n\
artifact mid/shared.txt reversible current chain gen/b.txt -> mid/shared.txt\n\
artifact out/beyond.txt reversible current chain gen/b.txt -> out/joint.txt -> out/beyond.txt\n\
artifact out/deep.txt reversible-with-compensation current chain gen/b.txt -> mid/shared.txt -> out/deep.txt\n\
artifact out/joint.txt irreversible current chain gen/b.txt -> out/joint.txt\n"
    );
}

// The refusals, BB-45.

#[test]
fn the_first_start_that_cannot_stand_names_the_refusal() {
    let stamp = batch_stamp();
    let state = batch_state();
    let err = recall_walk_multi(&stamp, &state, &starts(&["gen/a.txt", "ghost.txt"]), None)
        .expect_err("unknown second start refused");
    assert!(matches!(err, Error::Walk(_)), "{err:?}");
    assert_eq!(
        err.message(),
        "the recall cannot start from ghost.txt, which is not a file entry in the stamp"
    );
    // An artifact is not a file entry, and as the first start
    // it is named ahead of a sound second start.
    let err = recall_walk_multi(
        &stamp,
        &state,
        &starts(&["mid/only-a.txt", "gen/a.txt"]),
        None,
    )
    .expect_err("artifact start refused");
    assert_eq!(
        err.message(),
        "the recall cannot start from mid/only-a.txt, which is not a file entry in the stamp"
    );
}

// The classification point over a batch walk, BB-42 with
// BB-17 unchanged.

#[test]
fn an_unclassed_artifact_reached_by_two_starts_is_asked_about_once() {
    let text = format!(
        "stamp run-0043\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file gen/a.txt created {a}\n\
file gen/b.txt created {b}\n\
artifact out/open.txt unclassed {o} derived-from gen/a.txt, gen/b.txt\n",
        a = d('1'),
        b = d('2'),
        o = d('7'),
    );
    let stamp = parse_stamp(&py(&text), &py("stamp.txt")).expect("stamp");
    let state: HashMap<PyStr, PyStr> = [("out/open.txt", '7')]
        .iter()
        .map(|(path, c)| (py(path), py(&d(*c))))
        .collect();
    let mut source = SpySource::new();
    source.classification_answer = Some(obj(vec![
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
    ]));
    let entries = recall_walk_multi(
        &stamp,
        &state,
        &starts(&["gen/a.txt", "gen/b.txt"]),
        Some(&source),
    )
    .expect("batch walk with a classification");
    assert_eq!(source.classification_requests.borrow().len(), 1);
    let request = &source.classification_requests.borrow()[0];
    assert_eq!(
        request.get("artifact").and_then(|held| held.as_str()),
        Some(&py("out/open.txt"))
    );
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].rev_class, py("reversible"));
    assert!(entries[0].proposed);
    let text = format_recall_multi(&starts(&["gen/a.txt", "gen/b.txt"]), &entries);
    assert!(
        text.contains("artifact out/open.txt reversible proposed current"),
        "{text}"
    );
}

// The plan over a batch walk, BB-42 and BB-47.

#[test]
fn the_plan_asks_each_union_artifact_once_with_the_printed_chain() {
    let stamp = batch_stamp();
    let source = SpySource::new();
    let entries = build_plan_multi(
        &stamp,
        &batch_state(),
        &starts(&["gen/a.txt", "gen/b.txt"]),
        Some(&source),
    )
    .expect("batch plan");
    assert_eq!(entries.len(), 8);
    let requests = source.ranking_requests.borrow();
    // The plan asks the ranking point about an artifact only
    // when it carries two or more candidates, the seam's
    // standing rule. Six reversible artifacts at two and one
    // reversible-with-compensation at three make fifteen
    // questions, and the irreversible artifact, with its one
    // candidate, is never asked about.
    assert_eq!(requests.len(), 15);
    let mut pairs: Vec<(String, String)> = requests
        .iter()
        .map(|request| {
            (
                request
                    .get("artifact")
                    .and_then(|held| held.as_str())
                    .expect("artifact field")
                    .to_string(),
                request
                    .get("candidate")
                    .and_then(|held| held.as_str())
                    .expect("candidate field")
                    .to_string(),
            )
        })
        .collect();
    pairs.sort();
    pairs.dedup();
    assert_eq!(pairs.len(), 15, "each candidate is asked once");
    assert!(
        !pairs
            .iter()
            .any(|(artifact, _)| artifact == "out/joint.txt"),
        "a one candidate artifact is not asked about"
    );
    for request in requests.iter() {
        if request.get("artifact").and_then(|held| held.as_str()) == Some(&py("out/beyond.txt")) {
            assert_eq!(
                request.get("chain").and_then(|held| held.as_str()),
                Some(&py(
                    "gen/a.txt -> mid/a1.txt -> mid/a2.txt -> out/joint.txt -> out/beyond.txt"
                )),
                "the ranking request carries the chain the walk prints"
            );
        }
    }
}

#[test]
fn a_ranking_that_answers_only_the_later_starts_chain_leaves_the_artifact_unranked() {
    let stamp = batch_stamp();
    // The gate answers out/beyond.txt only under the short
    // road from gen/b.txt. While gen/a.txt stands first, the
    // printed chain is the long road, the gate never answers,
    // and the artifact stands unranked on BB-28 terms.
    let mut source = SpySource::new();
    source.ranking_gate = Some((
        "out/beyond.txt".to_string(),
        "gen/b.txt -> out/joint.txt -> out/beyond.txt".to_string(),
    ));
    let entries = build_plan_multi(
        &stamp,
        &batch_state(),
        &starts(&["gen/a.txt", "gen/b.txt"]),
        Some(&source),
    )
    .expect("batch plan");
    let text = format_plan_multi(&starts(&["gen/a.txt", "gen/b.txt"]), &entries);
    assert!(!text.contains(" score"), "{text}");
    // With the starts reversed the printed chain is the short
    // road, the same recorded answer now answers, and the
    // artifact stands ranked.
    let entries = build_plan_multi(
        &stamp,
        &batch_state(),
        &starts(&["gen/b.txt", "gen/a.txt"]),
        Some(&source),
    )
    .expect("batch plan reversed");
    let text = format_plan_multi(&starts(&["gen/b.txt", "gen/a.txt"]), &entries);
    assert!(
        text.contains("artifact out/beyond.txt reversible current chain gen/b.txt -> out/joint.txt -> out/beyond.txt\ncompensation rebuild-from-parents score 0.9\ncompensation remove-artifact score 0.9\n"),
        "{text}"
    );
}

// The command surface. The stamp and state the CLI cases
// share stand in a fresh directory per case.

fn cli_dir(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = common::test_dir(name);
    let stamp_text = format!(
        "stamp run-0042\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file gen/a.txt created {a}\n\
file gen/b.txt created {b}\n\
file gen/c.txt created {c}\n\
artifact mid/only-a.txt reversible {oa} derived-from gen/a.txt\n\
artifact mid/only-b.txt reversible {ob} derived-from gen/b.txt\n\
artifact mid/only-c.txt reversible-with-compensation {oc} derived-from gen/c.txt\n\
artifact mid/shared.txt reversible {sh} derived-from gen/a.txt, gen/b.txt\n\
artifact mid/a1.txt reversible {a1} derived-from gen/a.txt\n\
artifact mid/a2.txt reversible {a2} derived-from mid/a1.txt\n\
artifact out/joint.txt irreversible {jo} derived-from mid/a2.txt, gen/b.txt\n\
artifact out/deep.txt reversible-with-compensation {de} derived-from mid/shared.txt\n\
artifact out/beyond.txt reversible {be} derived-from out/joint.txt\n",
        a = d('1'),
        b = d('2'),
        c = d('3'),
        oa = d('a'),
        ob = d('b'),
        oc = d('c'),
        sh = d('d'),
        a1 = d('e'),
        a2 = d('f'),
        jo = d('4'),
        de = d('5'),
        be = d('6'),
    );
    let state_text = format!(
        "file mid/only-a.txt {}\nfile mid/only-b.txt {}\nfile mid/only-c.txt {}\n\
file mid/shared.txt {}\nfile mid/a1.txt {}\nfile mid/a2.txt {}\n\
file out/joint.txt {}\nfile out/deep.txt {}\nfile out/beyond.txt {}\n",
        d('a'),
        d('b'),
        d('c'),
        d('d'),
        d('e'),
        d('f'),
        d('4'),
        d('5'),
        d('6'),
    );
    let stamp = common::write(&dir, "stamp.txt", &stamp_text);
    let state = common::write(&dir, "state.txt", &state_text);
    (dir, stamp, state)
}

const TWO_START_RECALL: &str = "recall gen/a.txt\n\
recall gen/b.txt\n\
artifact mid/a1.txt reversible current chain gen/a.txt -> mid/a1.txt\n\
artifact mid/a2.txt reversible current chain gen/a.txt -> mid/a1.txt -> mid/a2.txt\n\
artifact mid/only-a.txt reversible current chain gen/a.txt -> mid/only-a.txt\n\
artifact mid/only-b.txt reversible current chain gen/b.txt -> mid/only-b.txt\n\
artifact mid/shared.txt reversible current chain gen/a.txt -> mid/shared.txt\n\
artifact out/beyond.txt reversible current chain gen/a.txt -> mid/a1.txt -> mid/a2.txt -> out/joint.txt -> out/beyond.txt\n\
artifact out/deep.txt reversible-with-compensation current chain gen/a.txt -> mid/shared.txt -> out/deep.txt\n\
artifact out/joint.txt irreversible current chain gen/a.txt -> mid/a1.txt -> mid/a2.txt -> out/joint.txt\n";

#[test]
fn the_recall_command_walks_from_two_starts() {
    let (dir, stamp, state) = cli_dir("batch-cli-two");
    let outcome = common::run_cli(
        &[
            "recall",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "gen/a.txt",
            "gen/b.txt",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    assert_eq!(outcome.stdout_text(), TWO_START_RECALL);
}

#[test]
fn the_plan_command_plans_from_two_starts() {
    let (dir, stamp, state) = cli_dir("batch-cli-plan");
    let outcome = common::run_cli(
        &[
            "plan",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "gen/a.txt",
            "gen/b.txt",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    let out = outcome.stdout_text();
    assert!(out.starts_with("plan gen/a.txt\nplan gen/b.txt\n"), "{out}");
    assert!(
        out.contains("artifact out/joint.txt irreversible current chain gen/a.txt -> mid/a1.txt -> mid/a2.txt -> out/joint.txt\ncompensation quarantine-artifact\n"),
        "{out}"
    );
    assert_eq!(
        out.lines()
            .filter(|line| line.starts_with("compensation "))
            .count(),
        16,
        "{out}"
    );
}

#[test]
fn a_repeated_start_contributes_nothing_beyond_its_first_occurrence() {
    let (dir, stamp, state) = cli_dir("batch-cli-repeat");
    let stamp_arg = stamp.to_str().expect("utf8");
    let state_arg = state.to_str().expect("utf8");
    let single = common::run_cli(&["recall", stamp_arg, state_arg, "gen/a.txt"], &dir);
    assert_eq!(single.code, 0);
    let repeated = common::run_cli(
        &["recall", stamp_arg, state_arg, "gen/a.txt", "gen/a.txt"],
        &dir,
    );
    assert_eq!(repeated.code, 0);
    assert_eq!(repeated.stdout_text(), single.stdout_text());
    // A repeat after another start changes nothing either.
    let trailed = common::run_cli(
        &[
            "recall",
            stamp_arg,
            state_arg,
            "gen/a.txt",
            "gen/b.txt",
            "gen/a.txt",
        ],
        &dir,
    );
    assert_eq!(trailed.code, 0);
    assert_eq!(trailed.stdout_text(), TWO_START_RECALL);
}

#[test]
fn an_unknown_start_refuses_the_batch_and_names_itself() {
    let (dir, stamp, state) = cli_dir("batch-cli-unknown");
    let stamp_arg = stamp.to_str().expect("utf8");
    let state_arg = state.to_str().expect("utf8");
    for order in [
        vec!["gen/a.txt", "ghost.txt"],
        vec!["ghost.txt", "gen/a.txt"],
    ] {
        let mut args = vec!["recall", stamp_arg, state_arg];
        args.extend(order.iter());
        let outcome = common::run_cli(&args, &dir);
        assert_eq!(outcome.code, 2, "{order:?}");
        assert_eq!(outcome.stdout_text(), "");
        assert!(
            outcome.stderr_text().contains(
                "the recall cannot start from ghost.txt, which is not a file entry in the stamp"
            ),
            "{}",
            outcome.stderr_text()
        );
    }
}

#[test]
fn a_listed_start_answers_in_its_position_among_the_starts() {
    let (dir, stamp, state) = cli_dir("batch-cli-listed");
    let stamp_arg = stamp.to_str().expect("utf8");
    let state_arg = state.to_str().expect("utf8");
    // A sound start ahead of the list stands, and the list
    // answers in its own voice at exit 1.
    let outcome = common::run_cli(
        &["recall", stamp_arg, state_arg, "gen/a.txt", "--", "--"],
        &dir,
    );
    assert_eq!(outcome.code, 1);
    assert_eq!(outcome.stdout_text(), "");
    assert!(
        outcome
            .stderr_text()
            .contains("an argument arrived as a list where a single file was expected"),
        "{}",
        outcome.stderr_text()
    );
    // An unknown start ahead of the list is examined first
    // and refuses in the walk voice at exit 2.
    let outcome = common::run_cli(
        &["recall", stamp_arg, state_arg, "ghost.txt", "--", "--"],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .stderr_text()
            .contains("the recall cannot start from ghost.txt"),
        "{}",
        outcome.stderr_text()
    );
    // A list ahead of a sound start answers first.
    let outcome = common::run_cli(
        &["recall", stamp_arg, state_arg, "--", "--", "gen/a.txt"],
        &dir,
    );
    assert_eq!(outcome.code, 1);
    assert_eq!(outcome.stdout_text(), "");
}

#[test]
fn a_trailing_separator_is_absorbed_at_a_batch_shape() {
    let (dir, stamp, state) = cli_dir("batch-cli-sep");
    let outcome = common::run_cli(
        &[
            "recall",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "gen/a.txt",
            "gen/b.txt",
            "--",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    assert_eq!(outcome.stdout_text(), TWO_START_RECALL);
}

#[test]
fn a_walk_with_no_start_is_not_assigned() {
    let (dir, stamp, state) = cli_dir("batch-cli-nostart");
    let outcome = common::run_cli(
        &[
            "recall",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
        ],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
}
