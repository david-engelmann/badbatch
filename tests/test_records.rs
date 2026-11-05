//! The record parser, ported test for test in substance from
//! tests/test_records.py at the Round 4 closure.

use badbatch::errors::Error;
use badbatch::pytext::PyStr;
use badbatch::records::{content_digest, parse_run_record};
use badbatch::stamps::{build_stamp, format_stamp};

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

const HEADER: &str = "run run-0007\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n";

const BODY: &str = "file src/report.py created\ncontent\nprint(\"report\")\nend\n\
file src/legacy.py deleted\n\
artifact build/report.html derived-from src/report.py class reversible\ncontent\n<html>report</html>\nend\n";

fn record(body: &str) -> Result<badbatch::records::RunRecord, Error> {
    parse_run_record(&py(&format!("{HEADER}{body}")), &py("record.txt"))
}

#[test]
fn a_well_formed_record_parses() {
    let parsed = record(BODY).expect("well-formed record");
    assert_eq!(parsed.run, py("run-0007"));
    assert_eq!(parsed.agent, py("writer-3"));
    assert_eq!(parsed.prompt_hash, py("b41d90"));
    assert_eq!(parsed.model, py("synth-writer-2"));
    assert_eq!(parsed.files.len(), 2);
    let report = &parsed.files[0];
    assert_eq!(report.path, py("src/report.py"));
    assert_eq!(report.action, py("created"));
    assert_eq!(report.content, Some(vec![py("print(\"report\")")]));
    let legacy = &parsed.files[1];
    assert_eq!(legacy.path, py("src/legacy.py"));
    assert_eq!(legacy.action, py("deleted"));
    assert_eq!(legacy.content, None);
    assert_eq!(parsed.artifacts.len(), 1);
    let artifact = &parsed.artifacts[0];
    assert_eq!(artifact.path, py("build/report.html"));
    assert_eq!(artifact.parents, vec![py("src/report.py")]);
    assert_eq!(artifact.rev_class, py("reversible"));
    assert_eq!(artifact.content, vec![py("<html>report</html>")]);
}

#[test]
fn entries_may_stand_in_any_order() {
    let parsed = record(
        "artifact build/report.html derived-from src/report.py class reversible\ncontent\n<html>report</html>\nend\n\
file src/report.py created\ncontent\nprint(\"report\")\nend\n",
    )
    .expect("entries in any order");
    assert_eq!(parsed.files.len(), 1);
    assert_eq!(parsed.artifacts.len(), 1);
}

#[test]
fn comments_and_blank_lines_outside_content_are_ignored() {
    let parsed = record(
        "# a note\n\nfile src/report.py created\ncontent\nprint(\"report\")\nend\n\n# another\n",
    )
    .expect("comments and blanks ignored");
    assert_eq!(parsed.files.len(), 1);
}

#[test]
fn content_lines_are_literal_until_the_exact_end_line() {
    let parsed = record("file src/a.txt created\ncontent\n\n# not a comment\n end \nend\n")
        .expect("literal content");
    assert_eq!(
        parsed.files[0].content,
        Some(vec![py(""), py("# not a comment"), py(" end ")])
    );
}

#[test]
fn header_defects_are_refused() {
    let missing_model = "run run-0007\nagent writer-3\nprompt-hash b41d90\n";
    let out_of_order = "run run-0007\nprompt-hash b41d90\nagent writer-3\nmodel synth-writer-2\n";
    let duplicated =
        "run run-0007\nagent writer-3\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n";
    let entry_first = "file src/report.py created\ncontent\nx\nend\n";
    let two_values =
        "run run-0007 extra\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n";
    for text in [
        missing_model,
        out_of_order,
        duplicated,
        entry_first,
        two_values,
    ] {
        let err =
            parse_run_record(&py(text), &py("record.txt")).expect_err("header defect refused");
        assert!(matches!(err, Error::Record(_)), "{err:?}");
    }
}

#[test]
fn malformed_records_are_refused() {
    let cases: Vec<String> = vec![
        // A line matching no entry form.
        format!("{HEADER}something else entirely\n"),
        // An unknown file action.
        format!("{HEADER}file src/a.py moved\ncontent\nx\nend\n"),
        // A created file with no content block.
        format!("{HEADER}file src/a.py created\n"),
        // A modified file whose block never closes.
        format!("{HEADER}file src/a.py modified\ncontent\nx\n"),
        // A deleted file carrying a content block.
        format!("{HEADER}file src/a.py deleted\ncontent\nx\nend\n"),
        // The same file path entered twice.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nfile src/a.py modified\ncontent\ny\nend\n"
        ),
        // An absolute path.
        format!("{HEADER}file /src/a.py created\ncontent\nx\nend\n"),
        // A parent reference in a path.
        format!("{HEADER}file src/../a.py created\ncontent\nx\nend\n"),
        // An artifact with an empty derivation list.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact out/a.html derived-from class reversible\ncontent\ny\nend\n"
        ),
        // An artifact naming no class.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact out/a.html derived-from src/a.py\ncontent\ny\nend\n"
        ),
        // An artifact with a class outside the three named classes.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact out/a.html derived-from src/a.py class forever\ncontent\ny\nend\n"
        ),
        // An artifact deriving from an undeclared parent.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact out/a.html derived-from src/ghost.py class reversible\ncontent\ny\nend\n"
        ),
        // An artifact with no content block.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact out/a.html derived-from src/a.py class reversible\n"
        ),
        // The same artifact path entered twice.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact out/a.html derived-from src/a.py class reversible\ncontent\ny\nend\nartifact out/a.html derived-from src/a.py class irreversible\ncontent\nz\nend\n"
        ),
        // An artifact path that is also a file path.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact src/a.py derived-from src/a.py class reversible\ncontent\ny\nend\n"
        ),
        // An artifact deriving from itself.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact out/a.html derived-from out/a.html class reversible\ncontent\ny\nend\n"
        ),
        // A two artifact cycle.
        format!(
            "{HEADER}file src/a.py created\ncontent\nx\nend\nartifact out/a.html derived-from out/b.html class reversible\ncontent\ny\nend\nartifact out/b.html derived-from out/a.html class irreversible\ncontent\nz\nend\n"
        ),
    ];
    for text in &cases {
        let err =
            parse_run_record(&py(text), &py("record.txt")).expect_err("malformed record refused");
        assert!(matches!(err, Error::Record(_)), "{err:?} for {text}");
    }
}

#[test]
fn a_refusal_names_the_source_and_the_defect() {
    let err = record("file src/a.py moved\ncontent\nx\nend\n").expect_err("refused");
    let message = err.message();
    assert!(message.contains("record.txt"), "{message}");
    assert!(message.contains("moved"), "{message}");
}

fn deep_record(count: usize, cycle: bool) -> String {
    let mut text = String::from(HEADER);
    text.push_str("file src/base.txt created\ncontent\nbase\nend\n");
    // Artifacts listed deepest first, step 0 deriving from the file.
    for i in (0..count).rev() {
        let parent = if i == 0 {
            if cycle {
                format!("step-{}.txt", count - 1)
            } else {
                "src/base.txt".to_string()
            }
        } else {
            format!("step-{}.txt", i - 1)
        };
        text.push_str(&format!(
            "artifact step-{i}.txt derived-from {parent} class reversible\ncontent\nbody {i}\nend\n"
        ));
    }
    text
}

#[test]
fn a_deep_chain_listed_deepest_first_stamps() {
    let parsed = parse_run_record(&py(&deep_record(1500, false)), &py("record.txt"))
        .expect("deep chain parses");
    let stamp = build_stamp(&parsed);
    assert_eq!(stamp.artifacts.len(), 1500);
    assert!(format_stamp(&stamp).starts_with("stamp run-0007\n"));
}

#[test]
fn a_deep_chain_closed_into_a_cycle_is_refused() {
    let err = parse_run_record(&py(&deep_record(1200, true)), &py("record.txt"))
        .expect_err("cycle refused");
    assert!(matches!(err, Error::Record(_)), "{err:?}");
}

#[test]
fn a_form_feed_inside_a_content_line_stays_in_the_line() {
    // The digest is sha256 of "al\x0cpha\n" as one line, computed
    // outside this crate.
    let parsed =
        record("file src/a.txt created\ncontent\nal\u{c}pha\nend\n").expect("form feed content");
    assert_eq!(parsed.files[0].content, Some(vec![py("al\u{c}pha")]));
    assert_eq!(
        content_digest(&parsed.files[0].content.clone().expect("content")),
        "56d842c0087ce43a5ab978a1187151c3fb4dfc122f4580d881060e204f4b3f29"
    );
}

#[test]
fn a_crlf_record_is_refused() {
    let text = HEADER.to_string().replace('\n', "\r\n")
        + "file src/a.py created\r\ncontent\r\nx\r\nend\r\n";
    let err = parse_run_record(&py(&text), &py("record.txt")).expect_err("CRLF record refused");
    assert!(matches!(err, Error::Record(_)), "{err:?}");
}
