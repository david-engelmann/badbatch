//! The CLI corpus: recorded invocations run against the real
//! binary. The oracle half was produced by the Python tool the
//! crate replaces; the clap half pins the help and usage
//! presentation the contract amendment hands to clap. The
//! per-source split is pinned so a case cannot silently change
//! which side of that line it sits on.

mod common;

use common::{load_corpus, run_corpus_case};

#[test]
fn the_corpus_runs_and_its_split_is_pinned() {
    let cases = load_corpus();
    let oracle = cases.iter().filter(|case| case.source == "oracle").count();
    let clap = cases.iter().filter(|case| case.source == "clap").count();
    assert_eq!((oracle, clap), (125, 30), "corpus source split");
    let mut failures = Vec::new();
    for case in &cases {
        if let Err(detail) = run_corpus_case(case) {
            failures.push(format!("{}: {detail}", case.name));
        }
    }
    assert!(
        failures.is_empty(),
        "corpus failures:\n{}",
        failures.join("\n")
    );
}
