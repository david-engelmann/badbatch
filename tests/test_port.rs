//! The Round 5 port pins (BB-30 to BB-41) that the Python
//! suites could not carry. Dead streams are closed pipes, the
//! only reliable failed write sink on this machine. Boundary
//! carriers are built at the exact byte sizes the probes
//! measured against the oracle.

mod common;

use std::path::Path;

use badbatch::judgment::load_fixture_file;
use badbatch::pytext::{PyStr, py_float_repr};

use common::{DeadStream, run_cli, run_cli_dead, test_dir, write, write_bytes};

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
        &[
            "recall",
            &stamp,
            &state,
            "src/a.py",
            "--fixtures",
            &fixtures,
        ],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert_eq!(
        outcome.stderr_text(),
        "error: fixture deep.json is not an object\n"
    );
}

#[test]
fn a_fixture_past_the_nesting_ceiling_is_the_uncaught_family() {
    let dir = test_dir("port-ceiling-over");
    let fixtures = fixture_dir_with(&dir, "deep.json", &deep_fixture(9992));
    let (stamp, state) = recall_args(&dir, &fixtures);
    let outcome = run_cli(
        &[
            "recall",
            &stamp,
            &state,
            "src/a.py",
            "--fixtures",
            &fixtures,
        ],
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
        &[
            "recall",
            &stamp,
            &state,
            "src/a.py",
            "--fixtures",
            &fixtures,
        ],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .stderr_text()
            .contains("is not a number between 0 and 1")
    );
}

#[test]
fn an_integer_past_the_digit_limit_is_the_uncaught_family() {
    let dir = test_dir("port-digits-over");
    let fixtures = fixture_dir_with(&dir, "digits.json", &digit_fixture(4301));
    let (stamp, state) = recall_args(&dir, &fixtures);
    let outcome = run_cli(
        &[
            "recall",
            &stamp,
            &state,
            "src/a.py",
            "--fixtures",
            &fixtures,
        ],
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

// The manifest placement pins, BB-30. The manifest is read
// as dependency sets, and every header that names a
// dependency set folds into it. A plain section contributes
// the crate names its body keys carry, a dotted subtable
// contributes the crate its last component names, and a
// target table contributes to the set it names, so a
// placement cannot hide inside a dotted subtable or behind
// a target gate.

use std::collections::BTreeSet;

/// The manifest split into its sections, each header with
/// the body lines that follow it.
fn manifest_sections(text: &str) -> Vec<(String, String)> {
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

/// Which dependency set one manifest header names, and the
/// crate a dotted header itself places, where it places one.
fn dependency_header(header: &str) -> Option<(&'static str, Option<String>)> {
    const KINDS: [&str; 3] = ["dev-dependencies", "build-dependencies", "dependencies"];
    for kind in KINDS {
        if header == kind {
            return Some((kind, None));
        }
        if let Some(crate_name) = header.strip_prefix(&format!("{kind}.")) {
            return Some((kind, Some(crate_name.trim_matches('"').to_string())));
        }
        if header.starts_with("target.") {
            let marker = format!(".{kind}");
            if let Some(at) = header.rfind(&marker) {
                let after = &header[at + marker.len()..];
                if after.is_empty() {
                    return Some((kind, None));
                }
                if let Some(crate_name) = after.strip_prefix('.') {
                    return Some((kind, Some(crate_name.trim_matches('"').to_string())));
                }
            }
        }
    }
    None
}

/// The crate names one plain dependency section's body keys
/// carry. A dotted key names its first component, so a key
/// written as a dotted path still names its crate.
fn body_crate_names(body: &str, out: &mut BTreeSet<String>) {
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            let first = key.trim().trim_matches('"').split('.').next().unwrap_or("");
            if !first.is_empty() {
                out.insert(first.to_string());
            }
        }
    }
}

/// The manifest's three dependency sets, shipped, dev, and
/// build, each holding the crate names placed in it by any
/// header form the manifest language allows.
fn dependency_sets(text: &str) -> (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>) {
    let mut shipped = BTreeSet::new();
    let mut dev = BTreeSet::new();
    let mut build = BTreeSet::new();
    for (header, body) in manifest_sections(text) {
        let Some((kind, implied)) = dependency_header(&header) else {
            continue;
        };
        let set = match kind {
            "dev-dependencies" => &mut dev,
            "build-dependencies" => &mut build,
            _ => &mut shipped,
        };
        if let Some(crate_name) = implied {
            set.insert(crate_name);
        } else {
            body_crate_names(&body, set);
        }
    }
    (shipped, dev, build)
}

#[test]
fn the_manifest_places_its_dependencies_as_the_contract_states() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("read manifest");
    // The section reader must account for the whole manifest,
    // or its sets are read from a partial text and prove
    // nothing. The package section stands as the witness. Its
    // body must still carry the version line.
    let sections = manifest_sections(&text);
    let package = sections
        .iter()
        .find(|(section, _)| section == "package")
        .map(|(_, body)| body.clone())
        .unwrap_or_default();
    assert!(
        package.contains("version = \"0.1.0\""),
        "the section reader dropped part of the manifest: {package}"
    );
    let (shipped, dev, build) = dependency_sets(&text);
    for crate_name in ["anyhow", "clap", "sha2", "thiserror"] {
        assert!(
            shipped.contains(crate_name),
            "{crate_name} must be a shipped dependency"
        );
    }
    assert!(
        shipped.contains("sha2"),
        "sha2 is shipped because stamp digests are product behavior"
    );
    for (set, name) in [(&shipped, "shipped"), (&build, "build")] {
        for crate_name in ["serde", "serde_json"] {
            assert!(
                !set.contains(crate_name),
                "no serde family crate may be a {name} dependency"
            );
        }
    }
    assert!(
        dev.contains("serde_json"),
        "serde_json is a dev dependency only"
    );
    // Positive controls. The reader must place a crate named
    // by a dotted subtable or a target table in the set the
    // table names, or its clean result on the real manifest
    // proves nothing.
    let (shipped, _, _) = dependency_sets("[dependencies.serde]\nversion = \"1\"\n");
    assert!(
        shipped.contains("serde"),
        "a dotted subtable must fold into its set"
    );
    let (shipped, _, _) = dependency_sets("[target.'cfg(unix)'.dependencies]\nserde = \"1\"\n");
    assert!(
        shipped.contains("serde"),
        "a target table must fold into its set"
    );
    let (_, _, build) = dependency_sets("[build-dependencies]\nserde_json = \"1\"\n");
    assert!(
        build.contains("serde_json"),
        "the build set must be read as its own set"
    );
    let (_, dev, _) = dependency_sets("[dev-dependencies.serde]\nversion = \"1\"\n");
    assert!(
        dev.contains("serde"),
        "a dotted dev subtable must fold into its set"
    );
}

// The Python removal and the import hygiene, BB-40 and BB-41.

#[test]
fn no_python_files_stand_in_the_tree() {
    // The walk reads the working tree from disk, because the
    // suite must also run in a fresh archive extraction that
    // carries no git state to list tracked files from. The
    // directories a contributor's own Python tooling keeps in
    // a checkout are skipped by name, since their contents are
    // not the tree this check guards.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    let mut bad = Vec::new();
    fn walk(dir: &Path, root: &Path, bad: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type().expect("type").is_dir() {
                if [
                    ".git",
                    "target",
                    "__pycache__",
                    ".venv",
                    "venv",
                    ".tox",
                    ".mypy_cache",
                    ".pytest_cache",
                    ".ruff_cache",
                    ".nox",
                ]
                .contains(&name.as_str())
                {
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
    assert!(bad.is_empty(), "python files stand in the tree: {bad:?}");
}

/// The crate roots one source text imports, resolving grouped,
/// nested, aliased, glob, and bare module forms.
///
/// A use statement is recognized at a line start, past any
/// indentation and past one pub marker, so a re-export and a
/// use standing inside a module or a function body are read
/// on the same terms as a use at the head of a file. Round 6
/// strengthened the scan on exactly those two forms, which the
/// port review recorded as evasions of the earlier reading.
fn imported_roots(text: &str) -> Vec<String> {
    let mut roots = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i == 0 || bytes[i - 1] == b'\n' {
            let mut j = i;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if bytes[j..].starts_with(b"pub ") {
                j += 4;
            }
            if bytes[j..].starts_with(b"use ") {
                // Take the statement up to its semicolon.
                let mut end = j + 4;
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
                let statement = &text[j + 4..end];
                let first = statement
                    .split(|c: char| c == ':' || c == '{' || c == ';' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .trim();
                if !first.is_empty() {
                    roots.push(first.to_string());
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    roots
}

/// Whether a source text names a crate root as the head of a
/// qualified path, the import-free form an import scan cannot
/// see. The root must stand at a path boundary, not inside a
/// longer identifier and not behind another path segment.
fn names_path_head(text: &str, root: &str) -> bool {
    let needle = format!("{root}::");
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(at) = text[from..].find(needle.as_str()).map(|found| found + from) {
        let boundary = at == 0 || {
            let prev = bytes[at - 1];
            !(prev.is_ascii_alphanumeric() || prev == b'_' || prev == b':')
        };
        if boundary {
            return true;
        }
        from = at + needle.len();
    }
    false
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
    // The two forms Round 6 closed. A re-export at a line
    // start and a use standing indented inside a body are read
    // as imports now.
    assert_eq!(
        imported_roots("pub use serde_json::Value;\n"),
        ["serde_json"]
    );
    assert_eq!(
        imported_roots("    use serde_json::Value;\n"),
        ["serde_json"]
    );
    assert_eq!(imported_roots("\tuse serde::Serialize;\n"), ["serde"]);
    assert_eq!(
        imported_roots("  pub use serde_json::Value;\n"),
        ["serde_json"]
    );
    // The path head scan must find a qualified path and must
    // not find the root inside a longer name or behind
    // another segment.
    assert!(names_path_head(
        "let x = serde_json::json!(1);\n",
        "serde_json"
    ));
    assert!(!names_path_head(
        "let x = myserde_json::json!(1);\n",
        "serde_json"
    ));
    assert!(!names_path_head(
        "let x = other::serde_json::json!(1);\n",
        "serde_json"
    ));
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
            // An import-free qualified path is the one form
            // the import scan cannot see, so the family's path
            // heads are refused in the shipped sources
            // outright. The family is not a shipped dependency,
            // so such a path could not compile either; the pin
            // keeps both readings inside the suite.
            for root in ["serde", "serde_json"] {
                assert!(
                    !names_path_head(&text, root),
                    "{} names {root} as a path head",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn anyhow_stays_confined_to_the_binary_entry() {
    // BB-30 confines anyhow to the binary entry, and until
    // Round 6 no test failed when a library source named it.
    // The confinement is pinned now in both of the scan's
    // forms, an import and a qualified path, with main.rs as
    // the one permitted file and as the witness that the pin
    // guards a real use.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut witness = false;
    for entry in std::fs::read_dir(&src).expect("read src") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("read source");
            let names_it = imported_roots(&text).iter().any(|root| root == "anyhow")
                || names_path_head(&text, "anyhow");
            if path.file_name().is_some_and(|name| name == "main.rs") {
                witness = names_it;
            } else {
                assert!(
                    !names_it,
                    "{} names anyhow outside the binary entry",
                    path.display()
                );
            }
        }
    }
    assert!(
        witness,
        "main.rs must name anyhow, or the confinement pin guards nothing"
    );
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
        outcome.stderr_text().starts_with("error: cannot read --, "),
        "{}",
        outcome.stderr_text()
    );
    let _ = stamp;
}

#[test]
fn a_separator_after_a_consumed_flag_still_names_the_stamp_slot() {
    let dir = test_dir("port-argv-flag-sep-stamp");
    let (stamp, state) = session_files(&dir);
    // With the fixtures option consumed first, the segment
    // [--, --, state, file] assigns the separator word itself
    // to the stamp slot, exactly as the segment at the head of
    // the stream does in the pin above. The stamp slot is the
    // one slot that never holds a list. Its share stands at
    // the head of its segment, the first separator of a
    // segment is discounted, and the next separator is the
    // share's value. The oracle's namespace table, read over
    // every token shape up to eight tokens drawn from values
    // and separators and the option pair, holds no shape that
    // puts a list in this slot, so the separator as its value
    // is the edge this slot carries.
    let outcome = run_cli(
        &[
            "recall",
            "--fixtures",
            dir.to_str().expect("utf8"),
            "--",
            "--",
            &state,
            "out/b.html",
        ],
        &dir,
    );
    assert_eq!(outcome.code, 2);
    assert!(
        outcome.stderr_text().starts_with("error: cannot read --, "),
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
    let outcome = run_cli(&["recall", &stamp, "--", "--fixtures", "somedir"], &dir);
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
        &[
            "recall",
            &stamp,
            &state,
            "--fixtures",
            dir.to_str().expect("utf8"),
            "--",
            "--",
        ],
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
    assert!(
        outcome
            .stdout_text()
            .starts_with("Walks a batch of stamped artifacts")
    );
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
            "recall",
            &stamp,
            &state,
            "src/a.py",
            "--fixtures",
            &ghost,
            "--fixtures",
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
    let outcome = run_cli(&["stamp", "missing.txt/"], &dir);
    assert_eq!(
        outcome.stderr_text(),
        "error: cannot read missing.txt, No such file or directory\n"
    );
}

#[test]
fn a_file_spelled_with_a_trailing_slash_or_dot_reads_as_the_file() {
    // BB-38 states the read opens the folded form, so these
    // spellings reach the file itself. A raw open fails each
    // with not a directory, which is how this pin failed
    // before the fold opened the read. The oracle answers
    // all three spellings with the one stamp document.
    let dir = test_dir("port-path-folded-read");
    write(&dir, "record.txt", SMALL_RECORD);
    let plain = run_cli(&["stamp", "record.txt"], &dir);
    assert_eq!(plain.code, 0);
    for spelling in ["record.txt/", "record.txt/.", "./record.txt"] {
        let outcome = run_cli(&["stamp", spelling], &dir);
        assert_eq!(outcome.code, 0, "{spelling}");
        assert_eq!(outcome.stdout_text(), plain.stdout_text(), "{spelling}");
        assert_eq!(outcome.stderr_text(), "", "{spelling}");
    }
}

#[test]
fn a_fixtures_directory_spelled_with_a_trailing_dot_loads() {
    // The fixture reads open the folded joined path, so a
    // directory spelled with a trailing dot component loads
    // its fixtures exactly as the plain spelling does.
    let dir = test_dir("port-path-folded-fixtures");
    let fixtures = fixture_dir_with(&dir, "deep.json", &deep_fixture(9991));
    let (stamp, state) = recall_args(&dir, &fixtures);
    let plain = run_cli(
        &[
            "recall",
            &stamp,
            &state,
            "src/a.py",
            "--fixtures",
            &fixtures,
        ],
        &dir,
    );
    let dotted = run_cli(
        &[
            "recall",
            &stamp,
            &state,
            "src/a.py",
            "--fixtures",
            &format!("{fixtures}/."),
        ],
        &dir,
    );
    assert_eq!(dotted.code, plain.code);
    assert_eq!(dotted.stdout_text(), plain.stdout_text());
    assert_eq!(dotted.stderr_text(), plain.stderr_text());
}

#[test]
fn a_fixture_file_spelled_with_a_trailing_dot_reads_through_the_fold() {
    // The joined paths the CLI builds always end in the file
    // name, so the fold at the fixture read itself is pinned
    // here at the library surface. A path ending in a dot
    // component reaches the file only through the folded
    // open; a raw open fails with not a directory and the
    // load answers that the fixture file cannot be read
    // instead of judging its content.
    let dir = test_dir("port-fixture-folded-read");
    write(&dir, "bad.json", "not json\n");
    let plain = dir.join("bad.json");
    let dotted = format!("{}/.", plain.to_str().expect("utf8"));
    let name = PyStr::from_str("bad.json");
    let error = load_fixture_file(dotted.as_bytes(), &name)
        .expect_err("an invalid fixture file refuses at validation");
    assert!(
        format!("{error}").contains("fixture file is not valid JSON, "),
        "{error}"
    );
    assert!(format!("{error}").ends_with("bad.json"), "{error}");
}

#[test]
fn a_utf8_refusal_names_the_path_as_written_under_a_folded_spelling() {
    let dir = test_dir("port-path-utf8-as-written");
    // The read opens the folded form, record.txt, but the
    // refusal that the document is not readable as UTF-8 text
    // names the path as written, ./record.txt, as BB-38
    // states. A port that named the folded form here would
    // print the name without the leading component.
    write_bytes(&dir, "record.txt", b"run run-0007\nagent \xffwriter\n");
    let outcome = run_cli(&["stamp", "./record.txt"], &dir);
    assert_eq!(outcome.code, 2);
    assert_eq!(
        outcome.stderr_text(),
        "error: ./record.txt is not readable as utf-8 text\n"
    );
}

#[test]
fn a_parse_refusal_names_the_path_as_written() {
    let dir = test_dir("port-path-as-written");
    write(&dir, "record.txt", "run run-0007\nagent writer-3\n");
    let outcome = run_cli(&["stamp", "./record.txt"], &dir);
    assert_eq!(outcome.code, 2);
    assert!(
        outcome
            .stderr_text()
            .contains("./record.txt is malformed, "),
        "{}",
        outcome.stderr_text()
    );
}

#[cfg(unix)]
#[test]
fn a_non_utf8_file_name_displays_with_surrogate_escapes() {
    let dir = test_dir("port-argv-bytes");
    let outcome = common::run_cli_bytes(&[b"stamp".as_slice(), b"gho\xffst.txt".as_slice()], &dir);
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
        vec![
            "recall",
            "s.txt",
            "--fixtures",
            fx,
            "t.txt",
            "src/a.py",
            "--fixtures",
            fx,
            "--",
        ],
        vec![
            "recall",
            "s.txt",
            "t.txt",
            "src/a.py",
            "--fixtures",
            fx,
            "--",
        ],
        vec![
            "recall",
            "--fixtures",
            fx,
            "s.txt",
            "t.txt",
            "src/a.py",
            "--fixtures",
            fx,
            "--",
        ],
    ] {
        let outcome = run_cli(&argv, &dir);
        assert_eq!(outcome.code, 2, "refused: {argv:?}");
        assert_eq!(outcome.stdout, b"", "no document: {argv:?}");
    }
    for argv in [
        vec![
            "recall",
            "s.txt",
            "--fixtures",
            fx,
            "t.txt",
            "src/a.py",
            "--",
        ],
        vec![
            "recall",
            "s.txt",
            "--fixtures",
            fx,
            "t.txt",
            "--fixtures",
            fx,
            "src/a.py",
        ],
        vec!["recall", "s.txt", "t.txt", "src/a.py", "--"],
    ] {
        let outcome = run_cli(&argv, &dir);
        assert_eq!(outcome.code, 0, "runs: {argv:?}");
        assert!(outcome.stdout_text().starts_with("recall src/a.py\n"));
    }
}
