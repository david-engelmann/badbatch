//! The Round 5 port pins (BB-30 to BB-41) that the Python
//! suites could not carry. Dead streams are closed pipes, the
//! only reliable failed write sink on this machine. Boundary
//! carriers are built at the exact byte sizes the probes
//! measured against the oracle.

mod common;

use std::path::Path;

use badbatch::pytext::py_float_repr;

use common::{DeadStream, run_cli, run_cli_dead, test_dir, write};

// The print and error discipline, BB-37.

const SMALL_RECORD: &str = "run run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\n\
file src/report.py created\ncontent\nprint(\"report\")\nend\n";

fn stamp_document_of_size(dir: &Path, target: usize) -> (String, usize) {
    // One file whose path length tunes the stamp document one
    // byte per character. Returns the record path and the
    // measured document length.
    let mut filler = 0usize;
    loop {
        let path = format!("f{}.txt", "x".repeat(filler));
        let record = format!(
            "run run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\n\
file {path} created\ncontent\nbody\nend\n"
        );
        let record_path = write(dir, "record.txt", &record);
        let outcome = run_cli(&["stamp", record_path.to_str().expect("utf8")], dir);
        assert_eq!(outcome.code, 0);
        let len = outcome.stdout.len();
        if len >= target {
            return (record_path.to_str().expect("utf8").to_string(), len);
        }
        filler += target - len;
    }
}

#[test]
fn a_document_under_the_buffer_divide_on_a_dead_stdout_exits_120() {
    let dir = test_dir("port-dead-small");
    let (record, len) = stamp_document_of_size(&dir, 8191);
    assert_eq!(len, 8191);
    let outcome = run_cli_dead(&["stamp", &record], &dir, DeadStream::Stdout);
    assert_eq!(outcome.code, 120);
    assert_eq!(
        outcome.stderr_text(),
        "badbatch could not print its document\n"
    );
}

#[test]
fn a_document_at_the_buffer_divide_on_a_dead_stdout_is_the_handler_line() {
    let dir = test_dir("port-dead-big");
    let (record, len) = stamp_document_of_size(&dir, 8192);
    assert_eq!(len, 8192);
    let outcome = run_cli_dead(&["stamp", &record], &dir, DeadStream::Stdout);
    assert_eq!(outcome.code, 2);
    assert_eq!(
        outcome.stderr_text(),
        "error: cannot read None, Broken pipe\n"
    );
}

#[test]
fn a_refusal_on_a_dead_stderr_exits_120() {
    let dir = test_dir("port-dead-refusal");
    let record = write(&dir, "record.txt", "run run-0007\nagent writer-3\n");
    let outcome = run_cli_dead(
        &["stamp", record.to_str().expect("utf8")],
        &dir,
        DeadStream::Stderr,
    );
    assert_eq!(outcome.code, 120);
    assert_eq!(outcome.stdout_text(), "");
}

#[test]
fn a_usage_error_on_a_dead_stderr_exits_120() {
    let dir = test_dir("port-dead-usage");
    let outcome = run_cli_dead(&[], &dir, DeadStream::Stderr);
    assert_eq!(outcome.code, 120);
}

#[test]
fn help_on_a_dead_stdout_exits_120() {
    let dir = test_dir("port-dead-help");
    let outcome = run_cli_dead(&["--help"], &dir, DeadStream::Stdout);
    assert_eq!(outcome.code, 120);
}

#[test]
fn a_refusal_on_a_dead_stdout_still_answers_two() {
    let dir = test_dir("port-dead-out-refusal");
    let record = write(&dir, "record.txt", "run run-0007\nagent writer-3\n");
    let outcome = run_cli_dead(
        &["stamp", record.to_str().expect("utf8")],
        &dir,
        DeadStream::Stdout,
    );
    assert_eq!(outcome.code, 2);
    assert!(outcome.stderr_text().starts_with("error: "));
}

#[test]
fn a_success_on_a_dead_stderr_is_unaffected() {
    let dir = test_dir("port-dead-err-success");
    let record = write(&dir, "record.txt", SMALL_RECORD);
    let outcome = run_cli_dead(
        &["stamp", record.to_str().expect("utf8")],
        &dir,
        DeadStream::Stderr,
    );
    assert_eq!(outcome.code, 0);
    assert!(outcome.stdout_text().starts_with("stamp run-0007\n"));
}

// The fixture reader ceilings, BB-34, through the real binary.

fn fixture_dir_with(dir: &Path, name: &str, text: &str) -> String {
    let fixtures = dir.join("fixtures");
    std::fs::create_dir_all(&fixtures).expect("fixtures dir");
    write(&fixtures, name, text);
    fixtures.to_str().expect("utf8").to_string()
}

fn recall_args(dir: &Path, fixtures: &str) -> (String, String) {
    let stamp = write(
        dir,
        "stamp.txt",
        &format!(
            "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\nfile src/a.py created {}\n",
            "a".repeat(64)
        ),
    );
    let state = write(dir, "state.txt", "");
    let _ = fixtures;
    (
        stamp.to_str().expect("utf8").to_string(),
        state.to_str().expect("utf8").to_string(),
    )
}

fn deep_fixture(depth: usize) -> String {
    format!("{}{}", "[".repeat(depth), "]".repeat(depth))
}

#[test]
fn a_fixture_at_the_nesting_ceiling_loads_and_is_refused_as_a_document() {
    let dir = test_dir("port-ceiling-loads");
    let fixtures = fixture_dir_with(&dir, "deep.json", &deep_fixture(9991));
    let (stamp, state) = recall_args(&dir, &fixtures);
    let outcome = run_cli(
        &["recall", &stamp, &state, "src/a.py", "--fixtures", &fixtures],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(outcome.stderr_text(), "error: fixture deep.json is not an object\n");
}

#[test]
fn a_fixture_past_the_nesting_ceiling_is_the_uncaught_family() {
    let dir = test_dir("port-ceiling-over");
    let fixtures = fixture_dir_with(&dir, "deep.json", &deep_fixture(9992));
    let (stamp, state) = recall_args(&dir, &fixtures);
    let outcome = run_cli(
        &["recall", &stamp, &state, "src/a.py", "--fixtures", &fixtures],
        &dir,
    );
    assert_eq!(outcome.code, 1);
    assert_eq!(
        outcome.stderr_text(),
        "badbatch: the fixture reader exceeded its nesting ceiling\n"
    );
}

fn digit_fixture(digits: usize) -> String {
    format!(
        "{{\"synthetic\": true, \"judgment_point\": \"classification\", \
\"request\": {{\"artifact\": \"build/report.html\", \"parents\": [\"src/report.py\"], \
\"digest\": \"{}\", \"status\": \"current\"}}, \
\"response\": {{\"kind\": \"choice\", \"choice\": \"reversible\", \
\"confidence\": 1{}, \"probabilities\": {{\"reversible\": 0.9, \
\"reversible-with-compensation\": 0.05, \"irreversible\": 0.05}}}}}}",
        "0".repeat(64),
        "0".repeat(digits - 1)
    )
}

#[test]
fn an_integer_at_the_digit_limit_is_read_and_refused_as_a_value() {
    let dir = test_dir("port-digits-at");
    let fixtures = fixture_dir_with(&dir, "digits.json", &digit_fixture(4300));
    let (stamp, state) = recall_args(&dir, &fixtures);
    let outcome = run_cli(
        &["recall", &stamp, &state, "src/a.py", "--fixtures", &fixtures],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert!(outcome.stderr_text().contains("is not a number between 0 and 1"));
}

#[test]
fn an_integer_past_the_digit_limit_is_the_uncaught_family() {
    let dir = test_dir("port-digits-over");
    let fixtures = fixture_dir_with(&dir, "digits.json", &digit_fixture(4301));
    let (stamp, state) = recall_args(&dir, &fixtures);
    let outcome = run_cli(
        &["recall", &stamp, &state, "src/a.py", "--fixtures", &fixtures],
        &dir,
    );
    assert_eq!(outcome.code, 1);
    assert_eq!(
        outcome.stderr_text(),
        "badbatch: the fixture reader exceeded its integer digit limit\n"
    );
}

// The trust threshold as a literal, BB-36.

#[test]
fn the_trust_threshold_is_the_contract_value() {
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("judgment.rs"),
    )
    .expect("read judgment.rs");
    assert!(
        source.contains("CONFIDENCE_THRESHOLD: f64 = 0.6"),
        "the threshold must be written as the literal 0.6 in the seam"
    );
    assert_eq!(badbatch::judgment::CONFIDENCE_THRESHOLD, 0.6);
}

// The manifest placement pins, BB-30. Sections are read with
// their dotted table names, so a placement cannot hide inside
// a dotted subtable.

fn manifest_sections() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .expect("read manifest");
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current = String::new();
    let mut body = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.starts_with("[[") {
            sections.push((current.clone(), std::mem::take(&mut body)));
            current = trimmed.trim_matches(['[', ']']).to_string();
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    sections.push((current, body));
    sections
}

#[test]
fn the_manifest_places_its_dependencies_as_the_contract_states() {
    let sections = manifest_sections();
    let body_of = |name: &str| {
        sections
            .iter()
            .find(|(section, _)| section == name)
            .map(|(_, body)| body.clone())
            .unwrap_or_default()
    };
    let dependencies = body_of("dependencies");
    for crate_name in ["anyhow", "clap", "sha2", "thiserror"] {
        assert!(
            dependencies.contains(&format!("{crate_name} =")),
            "{crate_name} must be a shipped dependency"
        );
    }
    assert!(
        dependencies.contains("sha2 ="),
        "sha2 is shipped because stamp digests are product behavior"
    );
    assert!(
        !dependencies.contains("serde"),
        "no serde family crate may be a shipped dependency: {dependencies}"
    );
    let dev = body_of("dev-dependencies");
    assert!(dev.contains("serde_json ="), "serde_json is a dev dependency only");
    for (section, body) in &sections {
        if section != "dev-dependencies" {
            assert!(
                !body.contains("serde_json"),
                "serde_json appears outside dev-dependencies in [{section}]"
            );
        }
    }
}

// The Python removal and the import hygiene, BB-40 and BB-41.

#[test]
fn no_python_is_tracked_in_the_tree() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    let mut bad = Vec::new();
    fn walk(dir: &Path, root: &Path, bad: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().expect("type").is_dir() {
                if [".git", "target", "__pycache__"].contains(&name.as_str()) {
                    continue;
                }
                walk(&path, root, bad);
            } else if name.ends_with(".py") || name == "pyproject.toml" {
                bad.push(
                    path.strip_prefix(root)
                        .expect("under root")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    walk(&root, &root, &mut bad);
    assert!(bad.is_empty(), "python remains tracked: {bad:?}");
}

/// The crate roots one source text imports, resolving grouped,
/// nested, aliased, glob, and bare module forms.
fn imported_roots(text: &str) -> Vec<String> {
    let mut roots = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 4 < bytes.len() {
        if (i == 0 || bytes[i - 1] == b'\n') && bytes[i..].starts_with(b"use ") {
            // Take the statement up to its semicolon.
            let mut end = i + 4;
            let mut depth = 0i32;
            while end < bytes.len() {
                match bytes[end] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    b';' if depth == 0 => break,
                    _ => {}
                }
                end += 1;
            }
            let statement = &text[i + 4..end];
            let first = statement
                .split(|c: char| c == ':' || c == '{' || c == ';' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .trim();
            if !first.is_empty() {
                roots.push(first.to_string());
            }
            i = end;
        } else {
            i += 1;
        }
    }
    roots
}

#[test]
fn the_import_scan_resolves_every_use_form() {
    // Positive controls. The scan must find the root in each
    // form, or its clean result on the real tree proves nothing.
    assert_eq!(imported_roots("use serde_json::Value;\n"), ["serde_json"]);
    assert_eq!(
        imported_roots("use serde::{Serialize, de::DeserializeOwned};\n"),
        ["serde"]
    );
    assert_eq!(
        imported_roots("use std::collections::{BTreeMap, HashMap};\n"),
        ["std"]
    );
    assert_eq!(imported_roots("use sha2::Digest as _;\n"), ["sha2"]);
    assert_eq!(imported_roots("use clap::*;\n"), ["clap"]);
    assert_eq!(imported_roots("use serde;\n"), ["serde"]);
}

#[test]
fn the_shipped_sources_never_import_the_serde_family() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in std::fs::read_dir(&src).expect("read src") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("read source");
            for root in imported_roots(&text) {
                assert!(
                    root != "serde" && root != "serde_json",
                    "{} imports {root}",
                    path.display()
                );
            }
        }
    }
}

// The printed float form, BB-35.

#[test]
fn the_float_printed_form_matches_the_python_repr_rule() {
    let cases: [(f64, &str); 8] = [
        (1.0, "1.0"),
        (0.5, "0.5"),
        (0.0001, "0.0001"),
        (0.00001, "1e-05"),
        (1e16, "1e+16"),
        (0.30000000000000004, "0.30000000000000004"),
        (2.5e-7, "2.5e-07"),
        (0.95, "0.95"),
    ];
    for (value, expected) in cases {
        assert_eq!(py_float_repr(value), expected, "{value}");
    }
}

// The separator distribution and the option surface, BB-39.
// Every expectation in this section was probed against the
// oracle's own argument parser before the port was written.

fn session_files(dir: &Path) -> (String, String) {
    let stamp = write(
        dir,
        "stamp.txt",
        &format!(
            "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file src/a.py created {a}\n\
artifact out/b.html reversible {b} derived-from src/a.py\n",
            a = "a".repeat(64),
            b = "b".repeat(64)
        ),
    );
    let state = write(
        dir,
        "state.txt",
        &format!("file out/b.html {}\n", "b".repeat(64)),
    );
    (
        stamp.to_str().expect("utf8").to_string(),
        state.to_str().expect("utf8").to_string(),
    )
}

#[test]
fn a_separator_led_segment_assigns_the_separator_as_a_value() {
    let dir = test_dir("port-argv-sep");
    let (stamp, state) = session_files(&dir);
    // recall -- -- state file parses with the stamp slot
    // holding the separator word itself, so the run refuses
    // reading a file literally named --.
    let outcome = run_cli(&["recall", "--", "--", &state, "out/b.html"], &dir);
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .stderr_text()
            .starts_with("error: cannot read --, "),
        "{}",
        outcome.stderr_text()
    );
    let _ = stamp;
}

#[test]
fn a_share_holding_a_separator_and_a_value_is_a_list() {
    let dir = test_dir("port-argv-list");
    let (stamp, state) = session_files(&dir);
    // recall stamp -- -- file leaves the state slot holding a
    // list, which surfaces as the uncaught family at the state
    // read, after the stamp read succeeds.
    let outcome = run_cli(&["recall", &stamp, "--", "--", "out/b.html"], &dir);
    assert_eq!(outcome.code, 1);
    assert_eq!(
        outcome.stderr_text(),
        "badbatch: an argument arrived as a list where a single path was expected\n"
    );
    let _ = state;
}

#[test]
fn a_list_in_the_start_slot_surfaces_at_the_walk() {
    let dir = test_dir("port-argv-start-list");
    let (stamp, state) = session_files(&dir);
    // recall stamp state -- -- leaves the start slot holding a
    // list, which surfaces as the uncaught family at the walk
    // start check, after both reads succeed.
    let outcome = run_cli(&["recall", &stamp, &state, "--", "--"], &dir);
    assert_eq!(outcome.code, 1);
    assert_eq!(
        outcome.stderr_text(),
        "badbatch: an argument arrived as a list where a single file was expected\n"
    );
}

#[test]
fn a_flag_after_the_first_separator_is_a_value() {
    let dir = test_dir("port-argv-flag-value");
    let (stamp, _state) = session_files(&dir);
    // recall stamp -- --fixtures dir assigns the flag word to
    // the state slot and dir to the start slot. The state read
    // opens the path as written and refuses.
    let outcome = run_cli(
        &["recall", &stamp, "--", "--fixtures", "somedir"],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .stderr_text()
            .starts_with("error: cannot read --fixtures, "),
        "{}",
        outcome.stderr_text()
    );
}

#[test]
fn a_trailing_separator_pair_can_still_name_the_separator() {
    let dir = test_dir("port-argv-sep-value");
    let (stamp, state) = session_files(&dir);
    // With the fixtures option consumed first, the segment
    // [stamp, state] closes, and the trailing segment [--, --]
    // assigns the separator word to the start slot, which the
    // walk then refuses as not a file entry.
    let outcome = run_cli(
        &["recall", &stamp, &state, "--fixtures", &dir.to_str().expect("utf8").to_string(), "--", "--"],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .stderr_text()
            .contains("which is not a file entry in the stamp"),
        "{}",
        outcome.stderr_text()
    );
}

#[test]
fn help_fires_and_is_blocked_exactly_where_the_oracle_put_it() {
    let dir = test_dir("port-argv-help");
    let outcome = run_cli(&["--help"], &dir);
    assert_eq!(outcome.code, 0);
    assert!(outcome
        .stdout_text()
        .starts_with("Stamp what a run left behind"));
    let outcome = run_cli(&["stamp", "record.txt", "extra", "--help"], &dir);
    assert_eq!(outcome.code, 0);
    let outcome = run_cli(&["--bogus", "--help"], &dir);
    assert_eq!(outcome.code, 0);
    let outcome = run_cli(&["bogus", "--help"], &dir);
    assert_eq!(outcome.code, 2);
    let outcome = run_cli(&["--help=x"], &dir);
    assert_eq!(outcome.code, 2);
    let outcome = run_cli(&["-hx"], &dir);
    assert_eq!(outcome.code, 0);
    let outcome = run_cli(&["stamp", "--", "--help"], &dir);
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .stderr_text()
            .starts_with("error: cannot read --help, "),
        "{}",
        outcome.stderr_text()
    );
}

#[test]
fn the_fixtures_option_abbreviates_and_repeats_take_the_last() {
    let dir = test_dir("port-argv-abbrev");
    let (stamp, state) = session_files(&dir);
    let fixtures = test_dir("port-argv-abbrev-fix");
    let fixtures_arg = fixtures.to_str().expect("utf8").to_string();
    let outcome = run_cli(
        &["recall", &stamp, &state, "src/a.py", "--fi", &fixtures_arg],
        &dir,
    );
    assert_eq!(outcome.code, 0);
    let ghost = dir.join("ghost").to_str().expect("utf8").to_string();
    let outcome = run_cli(
        &[
            "recall", &stamp, &state, "src/a.py", "--fixtures", &ghost, "--fixtures",
            &fixtures_arg,
        ],
        &dir,
    );
    assert_eq!(outcome.code, 0);
}

#[test]
fn an_empty_fixtures_value_loads_the_current_directory() {
    let dir = test_dir("port-argv-empty-fixtures");
    let (stamp, state) = session_files(&dir);
    // The empty value is not dropped. The current directory is
    // the fixture directory, and its junk.json refuses the run.
    write(&dir, "junk.json", "this is not json\n");
    let outcome = run_cli(
        &["recall", &stamp, &state, "src/a.py", "--fixtures", ""],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(
        outcome.stderr_text(),
        "error: fixture file is not valid JSON, junk.json\n"
    );
}

#[test]
fn a_negative_looking_value_is_a_value() {
    let dir = test_dir("port-argv-negative");
    let outcome = run_cli(&["stamp", "-1"], &dir);
    assert_eq!(outcome.code, 2);
    assert!(
        outcome.stderr_text().starts_with("error: cannot read -1, "),
        "{}",
        outcome.stderr_text()
    );
}

// The path discipline, BB-38.

#[test]
fn the_cannot_read_line_names_the_folded_path() {
    let dir = test_dir("port-path-fold");
    let outcome = run_cli(&["stamp", "./ghost.txt"], &dir);
    assert_eq!(
        outcome.stderr_text(),
        "error: cannot read ghost.txt, No such file or directory\n"
    );
    let outcome = run_cli(&["stamp", "sub//ghost.txt"], &dir);
    assert_eq!(
        outcome.stderr_text(),
        "error: cannot read sub/ghost.txt, No such file or directory\n"
    );
    let outcome = run_cli(&["stamp", ""], &dir);
    assert_eq!(
        outcome.stderr_text(),
        "error: cannot read ., Is a directory\n"
    );
    write(&dir, "ghost.txt", "run run-0007\n");
    let outcome = run_cli(&["stamp", "ghost.txt/"], &dir);
    assert_eq!(
        outcome.stderr_text(),
        "error: cannot read ghost.txt, Not a directory\n"
    );
}

#[test]
fn a_parse_refusal_names_the_path_as_written() {
    let dir = test_dir("port-path-as-written");
    write(&dir, "record.txt", "run run-0007\nagent writer-3\n");
    let outcome = run_cli(&["stamp", "./record.txt"], &dir);
    assert_eq!(outcome.code, 2);
    assert!(
        outcome.stderr_text().contains("./record.txt is malformed, "),
        "{}",
        outcome.stderr_text()
    );
}

#[cfg(unix)]
#[test]
fn a_non_utf8_file_name_displays_with_surrogate_escapes() {
    let dir = test_dir("port-argv-bytes");
    let outcome = common::run_cli_bytes(
        &[b"stamp".as_slice(), b"gho\xffst.txt".as_slice()],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(
        outcome.stderr_text(),
        "error: cannot read gho\\udcffst.txt, No such file or directory\n"
    );
}


#[test]
fn a_trailing_separator_closing_an_empty_segment_refuses() {
    // The oracle refuses a trailing separator when the segment
    // it closes holds no positional and a flag was consumed
    // earlier in the stream, even when every slot is filled.
    // The same streams without the separator, or with a
    // positional in the closing segment, run.
    let dir = test_dir("trailing-sep");
    let stamp = concat!(
        "stamp run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n",
        "file src/a.py created ",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    );
    write(&dir, "s.txt", stamp);
    write(&dir, "t.txt", "");
    std::fs::create_dir(dir.join("fx")).expect("fixtures dir");
    let fx = dir.join("fx");
    let fx = fx.to_str().expect("utf8 dir");
    for argv in [
        vec!["recall", "s.txt", "--fixtures", fx, "t.txt", "src/a.py", "--fixtures", fx, "--"],
        vec!["recall", "s.txt", "t.txt", "src/a.py", "--fixtures", fx, "--"],
        vec!["recall", "--fixtures", fx, "s.txt", "t.txt", "src/a.py", "--fixtures", fx, "--"],
    ] {
        let outcome = run_cli(&argv, &dir);
        assert_eq!(outcome.code, 2, "refused: {argv:?}");
        assert_eq!(outcome.stdout, b"", "no document: {argv:?}");
    }
    for argv in [
        vec!["recall", "s.txt", "--fixtures", fx, "t.txt", "src/a.py", "--"],
        vec!["recall", "s.txt", "--fixtures", fx, "t.txt", "--fixtures", fx, "src/a.py"],
        vec!["recall", "s.txt", "t.txt", "src/a.py", "--"],
    ] {
        let outcome = run_cli(&argv, &dir);
        assert_eq!(outcome.code, 0, "runs: {argv:?}");
        assert!(outcome.stdout_text().starts_with("recall src/a.py\n"));
    }
}
