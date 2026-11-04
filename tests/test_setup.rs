//! The setup invariants, re-expressed in crate form from
//! tests/test_setup.py at the Round 4 closure (BB-32). The
//! dash scan reads .rs where the Python form read .py, covers
//! extensionless files as well, and asserts its named anchor
//! files were actually scanned, so the scan cannot pass by
//! reading nothing.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const CANONICAL_PREFIX_SHA256: &str =
    "63d2f04005dc8eb111d3efca1be55c0911f9ad46bc7cf3c74d9f9908f4a86f11";

const EXPECTED_OBLIGATIONS: [&str; 5] = [
    "BB-S1. Every stamped artifact carries the agent, the prompt hash, and the model that made it, and a stamp missing any of those fields is rejected rather than stored incomplete.",
    "BB-S2. A recall walk from any stamped artifact lists every downstream artifact derived from it, and an artifact reachable through the stamp graph but missing from the walk is a defect the tests must catch.",
    "BB-S3. Every artifact in a recall result is sorted into exactly one reversibility class, reversible, reversible with compensation, or irreversible, and an unclassified artifact in the result is a defect.",
    "BB-S4. The recall walk reports only and changes nothing, and executing a recall is a separate act that never happens as a side effect of walking the graph.",
    "BB-S5. Every fixture and example in this repo is synthetic and built for this repo, and no real trace or session data appears anywhere in the tree.",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).expect("read file")
}

#[test]
fn the_contract_stub_obligations_exist() {
    let text = read("CONTRACT.md");
    let mut found: Vec<(String, u32, String)> = Vec::new();
    for line in text.split('\n') {
        let bytes = line.as_bytes();
        if bytes.len() > 6
            && bytes[0].is_ascii_uppercase()
            && bytes[1].is_ascii_uppercase()
            && &line[2..4] == "-S"
            && bytes[4].is_ascii_digit()
            && bytes[5] == b'.'
        {
            found.push((
                line[0..2].to_string(),
                (bytes[4] - b'0') as u32,
                line.to_string(),
            ));
        }
    }
    let texts: Vec<&str> = found.iter().map(|(_, _, line)| line.as_str()).collect();
    assert_eq!(
        texts,
        EXPECTED_OBLIGATIONS,
        "stub obligation text has changed without a contract amendment"
    );
    let numbers: Vec<u32> = found.iter().map(|(_, n, _)| *n).collect();
    assert_eq!(numbers, [1, 2, 3, 4, 5], "stub obligations must be exactly S1 to S5");
    for (prefix, _, line) in &found {
        assert_eq!(prefix, "BB", "obligation is not a BB obligation");
        let words = line.split_whitespace().count();
        assert!(words >= 20, "obligation too thin at {words} words");
    }
}

#[test]
fn the_agents_canonical_block_is_present() {
    let text = read("AGENTS.md");
    assert!(text.contains("repo annex below this line"));
    let prefix = text.split("## Repo annex").next().expect("prefix");
    let digest = format!("{:x}", Sha256::digest(prefix.as_bytes()));
    assert_eq!(
        digest, CANONICAL_PREFIX_SHA256,
        "canonical block above the annex line has drifted"
    );
    let annex = text.split("## Repo annex").nth(1).expect("annex");
    assert!(annex.contains("CONTRACT.md"));
}

#[test]
fn the_readme_status_reports_the_round_count() {
    let text = read("README.md");
    let counts = round_counts(&text);
    assert_eq!(
        counts,
        ["4"],
        "README status must report exactly one closed round count and it must be 4 of 100"
    );
}

/// Every digit run in a "Closed rounds N of 100" phrase whose
/// closing count is not followed by another digit. This is the
/// findall pin of the Python form, written without a regex
/// engine.
fn round_counts(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("Closed rounds ") {
        let after = &rest[start + "Closed rounds ".len()..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let tail = &after[digits.len()..];
            if let Some(tail) = tail.strip_prefix(" of 100") {
                let followed_by_digit =
                    tail.chars().next().is_some_and(|c| c.is_ascii_digit());
                if !followed_by_digit {
                    found.push(digits.clone());
                }
            }
        }
        rest = &rest[start + 1..];
    }
    found
}

const TEXT_SUFFIXES: [&str; 9] = [
    ".md", ".yml", ".yaml", ".rs", ".toml", ".txt", ".json", ".jsonl", ".lock",
];
const ANY_DEPTH_SKIP: [&str; 4] = [".git", "target", ".pytest_cache", "__pycache__"];
const ROOT_SKIP: [&str; 4] = [".venv", "venv", "node_modules", ".tox"];

fn collect_scanned(dir: &Path, depth: usize, out: &mut BTreeSet<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read dir");
    for entry in entries {
        let entry = entry.expect("entry");
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type().expect("file type");
        if file_type.is_dir() {
            if ANY_DEPTH_SKIP.contains(&name.as_str()) {
                continue;
            }
            if depth == 0 && ROOT_SKIP.contains(&name.as_str()) {
                continue;
            }
            collect_scanned(&path, depth + 1, out);
        } else if file_type.is_file() {
            let scanned = match path.extension().and_then(|e| e.to_str()) {
                None => true,
                Some(ext) => TEXT_SUFFIXES.contains(&format!(".{ext}").as_str()),
            };
            if scanned {
                out.insert(path);
            }
        }
    }
}

#[test]
fn no_em_or_en_dash_in_text_files() {
    let root = root();
    let mut scanned = BTreeSet::new();
    collect_scanned(&root, 0, &mut scanned);
    for anchor in ["CONTRACT.md", "Cargo.toml", "Cargo.lock", "src/lib.rs", "src/main.rs"] {
        assert!(
            scanned.contains(&root.join(anchor)),
            "anchor file {anchor} was not scanned"
        );
    }
    let em = "\u{2014}".as_bytes();
    let en = "\u{2013}".as_bytes();
    let mut bad = Vec::new();
    for path in &scanned {
        let bytes = std::fs::read(path).expect("read scanned file");
        if bytes.windows(3).any(|w| w == em || w == en) {
            bad.push(
                path.strip_prefix(&root)
                    .expect("under root")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    assert!(bad.is_empty(), "em or en dash found in {bad:?}");
}
