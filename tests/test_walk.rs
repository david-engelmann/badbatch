//! The recall walk, ported test for test in substance from
//! tests/test_walk.py at the Round 4 closure.

use std::collections::HashMap;

use badbatch::errors::Error;
use badbatch::pytext::PyStr;
use badbatch::records::parse_run_record;
use badbatch::stamps::{build_stamp, parse_stamp, Stamp};
use badbatch::walk::{format_recall, parse_current_state, recall_walk};

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

fn d(c: char) -> String {
    c.to_string().repeat(64)
}

/// The diamond session. gen/core.txt feeds left and right,
/// which both feed out/final.txt. gen/other.txt feeds only
/// out/side.txt.
fn diamond_stamp() -> Stamp {
    let text = format!(
        "stamp run-0031\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file gen/core.txt created {c}\n\
file gen/other.txt created {o}\n\
artifact gen/left.txt reversible {l} derived-from gen/core.txt\n\
artifact gen/right.txt reversible-with-compensation {r} derived-from gen/core.txt\n\
artifact out/final.txt irreversible {f} derived-from gen/left.txt, gen/right.txt\n\
artifact out/side.txt reversible {s} derived-from gen/other.txt\n",
        c = d('c'),
        o = d('0'),
        l = d('a'),
        r = d('b'),
        f = d('e'),
        s = d('d'),
    );
    parse_stamp(&py(&text), &py("stamp.txt")).expect("diamond stamp")
}

fn state_of(pairs: &[(&str, char)]) -> HashMap<PyStr, PyStr> {
    pairs
        .iter()
        .map(|(path, c)| (py(path), py(&d(*c))))
        .collect()
}

#[test]
fn state_lines_parse() {
    let state = parse_current_state(
        &py(&format!("file gen/left.txt {}\nfile x.txt {}\n", d('a'), d('b'))),
        &py("state.txt"),
    )
    .expect("state parses");
    assert_eq!(state.get(&py("gen/left.txt")), Some(&py(&d('a'))));
    assert_eq!(state.len(), 2);
}

#[test]
fn state_may_name_paths_the_stamp_never_mentions() {
    let stamp = diamond_stamp();
    let mut state = state_of(&[("gen/left.txt", 'a')]);
    state.insert(py("unrelated.txt"), py(&d('f')));
    let entries = recall_walk(&stamp, &state, &py("gen/core.txt"), None)
        .expect("walk tolerates extra state paths");
    assert_eq!(entries.len(), 3);
}

#[test]
fn malformed_state_is_refused() {
    let cases = vec![
        format!("file a.txt not-a-digest\n"),
        format!("file a.txt {}\nfile a.txt {}\n", d('a'), d('b')),
        "something else entirely\n".to_string(),
        "file a.txt none\n".to_string(),
    ];
    for text in &cases {
        let err = parse_current_state(&py(text), &py("state.txt"))
            .expect_err("malformed state refused");
        assert!(matches!(err, Error::State(_)), "{err:?} for {text}");
    }
}

#[test]
fn the_walk_lists_each_reachable_artifact_once() {
    let stamp = diamond_stamp();
    let state = state_of(&[
        ("gen/left.txt", 'a'),
        ("gen/right.txt", 'b'),
        ("out/final.txt", 'e'),
        ("out/side.txt", 'd'),
    ]);
    let entries = recall_walk(&stamp, &state, &py("gen/core.txt"), None).expect("walk");
    let paths: Vec<PyStr> = entries.iter().map(|e| e.path.clone()).collect();
    assert_eq!(entries.len(), 3);
    assert!(paths.contains(&py("gen/left.txt")));
    assert!(paths.contains(&py("gen/right.txt")));
    assert!(paths.contains(&py("out/final.txt")));
    // The start itself and the other file's artifacts stay out.
    assert!(!paths.contains(&py("gen/core.txt")));
    assert!(!paths.contains(&py("out/side.txt")));
}

#[test]
fn chains_are_shortest_and_break_ties_on_the_smallest_sequence() {
    let stamp = diamond_stamp();
    let state = state_of(&[("gen/left.txt", 'a'), ("gen/right.txt", 'b'), ("out/final.txt", 'e')]);
    let entries = recall_walk(&stamp, &state, &py("gen/core.txt"), None).expect("walk");
    let final_entry = entries
        .iter()
        .find(|e| e.path == py("out/final.txt"))
        .expect("final walked");
    assert_eq!(
        final_entry.chain,
        vec![py("gen/core.txt"), py("gen/left.txt"), py("out/final.txt")]
    );
}

#[test]
fn entries_sort_by_class_then_path() {
    let text = format!(
        "stamp run-0032\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file src.txt created {c}\n\
artifact bbb.txt reversible {x} derived-from src.txt\n\
artifact zzz.txt reversible {x} derived-from src.txt\n\
artifact mmm.txt reversible-with-compensation {x} derived-from src.txt\n\
artifact aaa.txt irreversible {x} derived-from src.txt\n",
        c = d('c'),
        x = d('a'),
    );
    let stamp = parse_stamp(&py(&text), &py("stamp.txt")).expect("stamp");
    let state = state_of(&[("bbb.txt", 'a'), ("zzz.txt", 'a'), ("mmm.txt", 'a'), ("aaa.txt", 'a')]);
    let entries = recall_walk(&stamp, &state, &py("src.txt"), None).expect("walk");
    let paths: Vec<String> = entries.iter().map(|e| e.path.to_string()).collect();
    assert_eq!(paths, ["bbb.txt", "zzz.txt", "mmm.txt", "aaa.txt"]);
}

#[test]
fn statuses_read_current_drifted_and_missing() {
    let stamp = diamond_stamp();
    let state = state_of(&[("gen/left.txt", 'a'), ("gen/right.txt", 'f')]);
    let entries = recall_walk(&stamp, &state, &py("gen/core.txt"), None).expect("walk");
    let by_path: HashMap<String, String> = entries
        .iter()
        .map(|e| (e.path.to_string(), e.status.to_string()))
        .collect();
    assert_eq!(by_path["gen/left.txt"], "current");
    assert_eq!(by_path["gen/right.txt"], "drifted");
    assert_eq!(by_path["out/final.txt"], "missing");
}

#[test]
fn the_walk_refuses_a_start_that_is_not_a_file_entry() {
    let stamp = diamond_stamp();
    let state = state_of(&[]);
    let err = recall_walk(&stamp, &state, &py("gen/left.txt"), None)
        .expect_err("artifact start refused");
    assert!(matches!(err, Error::Walk(_)), "{err:?}");
    let err = recall_walk(&stamp, &state, &py("ghost.txt"), None)
        .expect_err("ghost start refused");
    assert!(matches!(err, Error::Walk(_)), "{err:?}");
}

#[test]
fn a_lonely_file_walks_to_the_header_alone() {
    let text = format!(
        "stamp run-0033\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\nfile src/lonely.py created {}\n",
        d('a')
    );
    let stamp = parse_stamp(&py(&text), &py("stamp.txt")).expect("stamp");
    let entries = recall_walk(&stamp, &state_of(&[]), &py("src/lonely.py"), None)
        .expect("lonely walk");
    assert!(entries.is_empty());
    assert_eq!(
        format_recall(&py("src/lonely.py"), &entries),
        "recall src/lonely.py\n"
    );
}

#[test]
fn the_recall_text_lines_are_exact() {
    let stamp = diamond_stamp();
    let state = state_of(&[("gen/left.txt", 'a'), ("gen/right.txt", 'f')]);
    let entries = recall_walk(&stamp, &state, &py("gen/core.txt"), None).expect("walk");
    assert_eq!(
        format_recall(&py("gen/core.txt"), &entries),
        "recall gen/core.txt\n\
artifact gen/left.txt reversible current chain gen/core.txt -> gen/left.txt\n\
artifact gen/right.txt reversible-with-compensation drifted chain gen/core.txt -> gen/right.txt\n\
artifact out/final.txt irreversible missing chain gen/core.txt -> gen/left.txt -> out/final.txt\n"
    );
}

#[test]
fn a_parsed_stamp_walks_identically_to_a_built_one() {
    let record = parse_run_record(
        &py("run run-0031\nagent writer-3\nprompt-hash b41d90\nmodel synth-writer-2\n\
file gen/core.txt created\ncontent\ncore\nend\n\
file gen/other.txt created\ncontent\nother\nend\n\
artifact gen/left.txt derived-from gen/core.txt class reversible\ncontent\nleft\nend\n\
artifact gen/right.txt derived-from gen/core.txt class reversible-with-compensation\ncontent\nright\nend\n\
artifact out/final.txt derived-from gen/left.txt, gen/right.txt class irreversible\ncontent\nfinal\nend\n\
artifact out/side.txt derived-from gen/other.txt class reversible\ncontent\nside\nend\n"),
        &py("record.txt"),
    )
    .expect("record");
    let built = build_stamp(&record);
    let parsed = parse_stamp(
        &py(&badbatch::stamps::format_stamp(&built)),
        &py("stamp.txt"),
    )
    .expect("parsed stamp");
    let state = state_of(&[("gen/left.txt", 'a')]);
    let from_built = recall_walk(&built, &state, &py("gen/core.txt"), None).expect("walk");
    let from_parsed = recall_walk(&parsed, &state, &py("gen/core.txt"), None).expect("walk");
    assert_eq!(
        format_recall(&py("gen/core.txt"), &from_built),
        format_recall(&py("gen/core.txt"), &from_parsed)
    );
}
