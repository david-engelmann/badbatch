//! The committed example session regenerates byte for byte.
//! Ported from tests/test_examples.py at the Round 4 closure.

use std::collections::HashMap;
use std::path::PathBuf;

use badbatch::plan::{build_plan, format_plan};
use badbatch::pytext::PyStr;
use badbatch::records::parse_run_record;
use badbatch::stamps::{build_stamp, format_stamp, parse_stamp};
use badbatch::walk::{format_recall, parse_current_state, recall_walk};

fn py(text: &str) -> PyStr {
    PyStr::from_str(text)
}

fn read(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(name),
    )
    .expect("read example")
}

fn example_state() -> HashMap<PyStr, PyStr> {
    parse_current_state(
        &py(&read("current-state.txt")),
        &py("examples/current-state.txt"),
    )
    .expect("state parses")
}

#[test]
fn the_example_stamp_regenerates_byte_for_byte() {
    let record = parse_run_record(&py(&read("run-record.txt")), &py("examples/run-record.txt"))
        .expect("record");
    assert_eq!(format_stamp(&build_stamp(&record)), read("stamp.txt"));
}

#[test]
fn the_example_recall_regenerates_byte_for_byte() {
    let stamp = parse_stamp(&py(&read("stamp.txt")), &py("examples/stamp.txt")).expect("stamp");
    let entries = recall_walk(&stamp, &example_state(), &py("src/report.py"), None).expect("walk");
    assert_eq!(
        format_recall(&py("src/report.py"), &entries),
        read("recall.txt")
    );
}

#[test]
fn the_example_plan_regenerates_byte_for_byte() {
    let stamp = parse_stamp(&py(&read("stamp.txt")), &py("examples/stamp.txt")).expect("stamp");
    let entries = build_plan(&stamp, &example_state(), &py("src/report.py"), None).expect("plan");
    assert_eq!(
        format_plan(&py("src/report.py"), &entries),
        read("plan.txt")
    );
}

#[test]
fn the_example_session_shows_drift_from_the_other_start_file() {
    let stamp = parse_stamp(&py(&read("stamp.txt")), &py("examples/stamp.txt")).expect("stamp");
    let entries = recall_walk(&stamp, &example_state(), &py("src/data.csv"), None).expect("walk");
    let by_path: HashMap<PyStr, &badbatch::walk::WalkEntry> =
        entries.iter().map(|e| (e.path.clone(), e)).collect();
    assert_eq!(by_path[&py("build/chart.png")].status, py("drifted"));
    assert_eq!(by_path[&py("dist/bundle.zip")].status, py("current"));
    assert_eq!(
        by_path[&py("build/chart.png")].chain,
        vec![py("src/data.csv"), py("build/chart.png")]
    );
}
