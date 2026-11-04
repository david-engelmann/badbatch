//! The command line tool and its discipline, ported test for
//! test in substance from tests/test_cli.py at the Round 4
//! closure. The Python in-process main() calls run the compiled
//! binary here, and the module entry test runs the binary
//! itself, which is the crate's only entry point.

mod common;

use std::path::Path;

use badbatch::pytext::PyStr;
use badbatch::records::parse_run_record;
use badbatch::stamps::{build_stamp, format_stamp};

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

const RECORD: &str = "run run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\n\
file src/report.py created\ncontent\nprint(\"report\")\nend\n\n\
file src/legacy.py deleted\n\n\
artifact build/report.html derived-from src/report.py class reversible\ncontent\n<html>report</html>\nend\n";

fn stamp_text(record_text: &str) -> String {
    let record = parse_run_record(&py(record_text), &py("record.txt")).expect("record");
    format_stamp(&build_stamp(&record))
}

#[test]
fn the_stamp_command_prints_the_stamp_and_exits_zero() {
    let dir = common::test_dir("cli-stamp");
    let record = common::write(&dir, "record.txt", RECORD);
    let outcome = common::run_cli(&["stamp", record.to_str().expect("utf8")], &dir);
    assert_eq!(outcome.code, 0);
    let out = outcome.stdout_text();
    assert!(out.starts_with("stamp run-0007\nagent writer-3\n"), "{out}");
    assert!(out.contains("file src/legacy.py deleted none\n"), "{out}");
}

#[test]
fn the_recall_command_prints_the_walk() {
    let dir = common::test_dir("cli-recall");
    let stamp = common::write(&dir, "stamp.txt", &stamp_text(RECORD));
    let record = parse_run_record(&py(RECORD), &py("record.txt")).expect("record");
    let digest = build_stamp(&record).artifacts[0].digest.clone();
    let state = common::write(
        &dir,
        "state.txt",
        &format!("file build/report.html {digest}\n"),
    );
    let outcome = common::run_cli(
        &[
            "recall",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "src/report.py",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    assert_eq!(
        outcome.stdout_text(),
        "recall src/report.py\n\
artifact build/report.html reversible current chain src/report.py -> build/report.html\n"
    );
}

#[test]
fn a_malformed_record_exits_two_with_one_error_line() {
    let dir = common::test_dir("cli-malformed");
    let record = common::write(&dir, "record.txt", "run run-0007\nagent writer-3\n");
    let outcome = common::run_cli(&["stamp", record.to_str().expect("utf8")], &dir);
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
    let err = outcome.stderr_text();
    assert!(err.starts_with("error: "), "{err}");
    assert_eq!(err.matches('\n').count(), 1);
    assert!(!err.contains("Traceback"));
}

#[test]
fn a_missing_input_file_exits_two() {
    let dir = common::test_dir("cli-missing");
    let ghost = dir.join("ghost.txt");
    let outcome = common::run_cli(&["stamp", ghost.to_str().expect("utf8")], &dir);
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
    assert!(outcome.stderr_text().contains("cannot read"));
}

#[test]
fn a_recall_from_a_non_file_start_exits_two() {
    let dir = common::test_dir("cli-nonfile");
    let stamp = common::write(&dir, "stamp.txt", &stamp_text(RECORD));
    let state = common::write(&dir, "state.txt", "");
    let outcome = common::run_cli(
        &[
            "recall",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "build/report.html",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stdout_text(), "");
}

#[test]
fn outputs_are_byte_identical_across_runs_and_input_order() {
    let dir = common::test_dir("cli-identical");
    let shuffled = RECORD.replace(
        "file src/report.py created\ncontent\nprint(\"report\")\nend\n\nfile src/legacy.py deleted",
        "file src/legacy.py deleted\n\nfile src/report.py created\ncontent\nprint(\"report\")\nend",
    );
    assert_ne!(shuffled, RECORD);
    let first = common::write(&dir, "record-a.txt", RECORD);
    let second = common::write(&dir, "record-b.txt", &shuffled);
    let out_first = common::run_cli(&["stamp", first.to_str().expect("utf8")], &dir);
    let out_second = common::run_cli(&["stamp", second.to_str().expect("utf8")], &dir);
    let out_third = common::run_cli(&["stamp", first.to_str().expect("utf8")], &dir);
    assert_eq!(out_first.code, 0);
    assert_eq!(out_first.stdout, out_second.stdout);
    assert_eq!(out_first.stdout, out_third.stdout);
}

#[test]
fn the_commands_modify_none_of_their_inputs() {
    let dir = common::test_dir("cli-readonly");
    let record = common::write(&dir, "record.txt", RECORD);
    let stamp = common::write(&dir, "stamp.txt", &stamp_text(RECORD));
    let state = common::write(&dir, "state.txt", "");
    let paths: Vec<&Path> = vec![&record, &stamp, &state];
    let before: Vec<Vec<u8>> = paths.iter().map(|p| common::read(p)).collect();
    let outcome = common::run_cli(&["stamp", record.to_str().expect("utf8")], &dir);
    assert_eq!(outcome.code, 0);
    let outcome = common::run_cli(
        &[
            "recall",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "src/report.py",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    for (path, content) in paths.iter().zip(before.iter()) {
        assert_eq!(&common::read(path), content);
    }
}

#[test]
fn a_recall_with_no_downstream_prints_the_header_alone() {
    let dir = common::test_dir("cli-unrelated");
    let altered = RECORD
        .replace("artifact build/report.html", "artifact build/unrelated.html")
        .replace("derived-from src/report.py", "derived-from src/legacy.py");
    let record = common::write(&dir, "record.txt", &altered);
    let record_obj = parse_run_record(
        &py(&std::fs::read_to_string(&record).expect("read")),
        &py("record.txt"),
    )
    .expect("record");
    let stamp = common::write(
        &dir,
        "stamp.txt",
        &format_stamp(&build_stamp(&record_obj)),
    );
    let state = common::write(&dir, "state.txt", "");
    let outcome = common::run_cli(
        &[
            "recall",
            stamp.to_str().expect("utf8"),
            state.to_str().expect("utf8"),
            "src/report.py",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    assert_eq!(outcome.stdout_text(), "recall src/report.py\n");
}

#[test]
fn the_binary_entry_point_runs() {
    let dir = common::test_dir("cli-entry");
    let record = common::write(&dir, "record.txt", RECORD);
    let outcome = common::run_cli(&["stamp", record.to_str().expect("utf8")], &dir);
    assert_eq!(outcome.code, 0);
    assert!(outcome.stdout_text().starts_with("stamp run-0007\n"));
    assert_eq!(outcome.stderr_text(), "");
}
