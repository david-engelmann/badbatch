//! The recall walk over a stamp graph against a recorded
//! current state.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use crate::errors::{Error, Result};
use crate::judgment::{JudgmentSource, check_answer, classification_request, trusted};
use crate::pytext::PyStr;
use crate::records::{CLASSES, PathErrorKind, UNCLASSED, check_path};
use crate::stamps::{Stamp, class_word_refusal, is_digest};

#[derive(Clone, Debug, PartialEq)]
pub struct WalkEntry {
    pub path: PyStr,
    pub rev_class: PyStr,
    pub status: PyStr,
    pub chain: Vec<PyStr>,
    pub proposed: bool,
}

/// Parse one recorded current state into a path to digest
/// mapping.
pub fn parse_current_state(text: &PyStr, source: &PyStr) -> Result<HashMap<PyStr, PyStr>> {
    let mut state: HashMap<PyStr, PyStr> = HashMap::new();
    for raw_line in text.split_point(b'\n' as u32) {
        let stripped = raw_line.strip();
        if stripped.is_empty() || stripped.starts_with_point(b'#' as u32) {
            continue;
        }
        let parts = stripped.split_whitespace();
        if parts.len() != 3 || parts[0] != "file" {
            return Err(Error::State(format!(
                "{source} is malformed, the line {} matches no current \
                 state line form",
                crate::pytext::py_repr(&stripped)
            )));
        }
        let path = parts[1].clone();
        let digest = parts[2].clone();
        check_path(&path, source, PathErrorKind::State)?;
        if !is_digest(&digest) {
            return Err(Error::State(format!(
                "{source} is malformed, the line for {path} carries the \
                 digest {}, which is not sixty-four lowercase hex characters",
                crate::pytext::py_repr(&digest)
            )));
        }
        if state.contains_key(&path) {
            return Err(Error::State(format!(
                "{source} is malformed, the path {path} is stated twice"
            )));
        }
        state.insert(path, digest);
    }
    Ok(state)
}

#[derive(Clone, PartialEq, Eq)]
struct HeapItem {
    length: usize,
    chain: Vec<PyStr>,
    node: PyStr,
}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // The heap pops the smallest item, as the oracle's heap
        // pops the smallest tuple.
        other
            .length
            .cmp(&self.length)
            .then_with(|| other.chain.cmp(&self.chain))
            .then_with(|| other.node.cmp(&self.node))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Every downstream artifact of one stamped file, sorted for
/// report.
///
/// The walk follows derivation links from the start file
/// through artifacts, keeps the shortest chain to each artifact
/// with ties broken by the lexicographically smallest chain,
/// and reports only.
///
/// An artifact the stamp leaves unclassed is asked about once,
/// when a judgment source is given, and a trusted answer
/// proposes its class for this result alone. Without a trusted
/// proposal the walk refuses that artifact with the stamp
/// parser's class refusal, so the recall behaves exactly as it
/// behaved before the seam existed.
pub fn recall_walk(
    stamp: &Stamp,
    state: &HashMap<PyStr, PyStr>,
    start: &PyStr,
    player: Option<&dyn JudgmentSource>,
) -> Result<Vec<WalkEntry>> {
    if !stamp.files.iter().any(|entry| &entry.path == start) {
        return Err(Error::Walk(format!(
            "the recall cannot start from {start}, which is not a \
             file entry in the stamp"
        )));
    }
    let mut children: HashMap<PyStr, Vec<PyStr>> = HashMap::new();
    for artifact in &stamp.artifacts {
        for parent in &artifact.parents {
            children
                .entry(parent.clone())
                .or_default()
                .push(artifact.path.clone());
        }
    }
    let mut best: HashMap<PyStr, Vec<PyStr>> = HashMap::new();
    best.insert(start.clone(), vec![start.clone()]);
    let mut heap = BinaryHeap::new();
    heap.push(HeapItem {
        length: 1,
        chain: vec![start.clone()],
        node: start.clone(),
    });
    while let Some(item) = heap.pop() {
        let chain = item.chain;
        if best.get(&item.node) != Some(&chain) {
            continue;
        }
        let mut sorted_children = children.get(&item.node).cloned().unwrap_or_default();
        sorted_children.sort();
        for child in sorted_children {
            let mut candidate = chain.clone();
            candidate.push(child.clone());
            let improves = match best.get(&child) {
                None => true,
                Some(known) => {
                    (candidate.len(), &candidate) < (known.len(), known)
                }
            };
            if improves {
                best.insert(child.clone(), candidate.clone());
                heap.push(HeapItem {
                    length: candidate.len(),
                    chain: candidate,
                    node: child,
                });
            }
        }
    }
    let mut ordered_paths: Vec<&PyStr> = best.keys().collect();
    ordered_paths.sort();
    let mut entries: Vec<WalkEntry> = Vec::new();
    for path in ordered_paths {
        if path == start {
            continue;
        }
        let artifact = stamp
            .artifact_by_path(path)
            .expect("every reached non start path is an artifact");
        let status = match state.get(path) {
            None => "missing",
            Some(current) if *current == artifact.digest => "current",
            Some(_) => "drifted",
        };
        let mut rev_class = artifact.rev_class.clone();
        let mut proposed = false;
        if rev_class == UNCLASSED {
            let answer = match player {
                Some(source) => {
                    let request = classification_request(artifact, status);
                    check_answer(source.judge(&request))
                }
                None => None,
            };
            if !trusted(answer.as_ref()) {
                return Err(class_word_refusal(
                    &stamp.source,
                    &format!("the artifact line for {path}"),
                    &PyStr::from_str(UNCLASSED),
                ));
            }
            rev_class = answer
                .as_ref()
                .and_then(|held| held.get("choice"))
                .and_then(|choice| choice.as_str())
                .expect("a trusted answer carries a choice")
                .clone();
            proposed = true;
        }
        entries.push(WalkEntry {
            path: path.clone(),
            rev_class,
            status: PyStr::from_str(status),
            chain: best[path].clone(),
            proposed,
        });
    }
    entries.sort_by(|left, right| {
        let left_order = CLASSES.iter().position(|known| left.rev_class == *known);
        let right_order = CLASSES.iter().position(|known| right.rev_class == *known);
        left_order
            .cmp(&right_order)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(entries)
}

/// Render a recall result as the deterministic document text.
pub fn format_recall(start: &PyStr, entries: &[WalkEntry]) -> String {
    let mut lines: Vec<String> = vec![format!("recall {start}")];
    for entry in entries {
        let chain = entry
            .chain
            .iter()
            .map(|part| part.to_display())
            .collect::<Vec<String>>()
            .join(" -> ");
        let marker = if entry.proposed { " proposed" } else { "" };
        lines.push(format!(
            "artifact {} {}{} {} chain {}",
            entry.path, entry.rev_class, marker, entry.status, chain
        ));
    }
    lines.join("\n") + "\n"
}
