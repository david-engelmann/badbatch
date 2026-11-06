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
    start_index: usize,
    length: usize,
    chain: Vec<PyStr>,
    node: PyStr,
}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // The heap pops the smallest item, as the oracle's heap
        // pops the smallest tuple. The start index stands ahead
        // of the length and the chain, so one start's frontier
        // settles before the next start's. Ownership of a chain
        // does not rest on the pop order. It rests on the best
        // chains standing per start and on the assembly taking
        // each artifact's chain from its earliest start.
        other
            .start_index
            .cmp(&self.start_index)
            .then_with(|| other.length.cmp(&self.length))
            .then_with(|| other.chain.cmp(&self.chain))
            .then_with(|| other.node.cmp(&self.node))
    }
}

/// The refusal a start that names no file entry in the stamp
/// earns, in the walk's own voice.
pub fn start_refusal(start: &PyStr) -> Error {
    Error::Walk(format!(
        "the recall cannot start from {start}, which is not a \
         file entry in the stamp"
    ))
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
    recall_walk_multi(stamp, state, std::slice::from_ref(start), player)
}

/// Every downstream artifact of one or more stamped files,
/// sorted for report, under BB-42 to BB-45.
///
/// The walk runs one Dijkstra pass seeded with every start at
/// once. Each start's exploration is its own, so a path from a
/// later start may pass through an artifact an earlier start
/// reaches, and the chain an artifact carries is the chain its
/// earliest start in argument order reaches it by, as BB-43
/// states. A start naming no file entry in the stamp refuses
/// the whole walk, naming the first such start in argument
/// order.
pub fn recall_walk_multi(
    stamp: &Stamp,
    state: &HashMap<PyStr, PyStr>,
    starts: &[PyStr],
    player: Option<&dyn JudgmentSource>,
) -> Result<Vec<WalkEntry>> {
    for start in starts {
        if !stamp.files.iter().any(|entry| &entry.path == start) {
            return Err(start_refusal(start));
        }
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
    // The best chain is kept per start and node, because a
    // later start's path to an artifact an earlier start
    // claims still has to be walked through when it is the
    // only road to an artifact further down.
    let mut best: HashMap<(usize, PyStr), Vec<PyStr>> = HashMap::new();
    let mut heap = BinaryHeap::new();
    for (index, start) in starts.iter().enumerate() {
        best.insert((index, start.clone()), vec![start.clone()]);
        heap.push(HeapItem {
            start_index: index,
            length: 1,
            chain: vec![start.clone()],
            node: start.clone(),
        });
    }
    while let Some(item) = heap.pop() {
        let chain = item.chain;
        if best.get(&(item.start_index, item.node.clone())) != Some(&chain) {
            continue;
        }
        let mut sorted_children = children.get(&item.node).cloned().unwrap_or_default();
        sorted_children.sort();
        for child in sorted_children {
            let mut candidate = chain.clone();
            candidate.push(child.clone());
            let key = (item.start_index, child.clone());
            let improves = match best.get(&key) {
                None => true,
                Some(known) => (candidate.len(), &candidate) < (known.len(), known),
            };
            if improves {
                best.insert(key, candidate.clone());
                heap.push(HeapItem {
                    start_index: item.start_index,
                    length: candidate.len(),
                    chain: candidate,
                    node: child,
                });
            }
        }
    }
    let mut reached: Vec<&PyStr> = Vec::new();
    for (_, node) in best.keys() {
        if !reached.contains(&node) {
            reached.push(node);
        }
    }
    reached.sort();
    let mut entries: Vec<WalkEntry> = Vec::new();
    for path in reached {
        let Some(artifact) = stamp.artifact_by_path(path) else {
            // A reached path that is no artifact is one of the
            // start files themselves, which the walk never
            // lists.
            continue;
        };
        let owner = (0..starts.len())
            .find(|index| best.contains_key(&(*index, path.clone())))
            .expect("a reached artifact is reached by some start");
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
            chain: best[&(owner, path.clone())].clone(),
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
    format_recall_multi(std::slice::from_ref(start), entries)
}

/// Render a recall result for one or more starts, one header
/// line per start in argument order, under BB-44.
pub fn format_recall_multi(starts: &[PyStr], entries: &[WalkEntry]) -> String {
    let mut lines: Vec<String> = starts
        .iter()
        .map(|start| format!("recall {start}"))
        .collect();
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
