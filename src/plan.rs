//! The execution plan for a recall, built over the recall
//! walk.
//!
//! The plan is a document a human reviewer reads before any
//! recall is executed. It states, for every artifact the walk
//! lists, the candidate compensations that could undo the
//! artifact and the order a reviewer should consider them in.
//! The candidate set is a deterministic function of the
//! artifact class and status. A recorded judgment may rank the
//! candidates of one artifact by score, and that ranking
//! orders only the options the reviewer sees. The plan executes
//! nothing, approves nothing, and triggers nothing.

use std::collections::HashMap;

use crate::errors::{Error, Result};
use crate::judgment::{
    CANDIDATES, JudgmentSource, check_ranking_answer, ranking_request, trusted_score,
};
use crate::pytext::{PyStr, py_float_repr};
use crate::stamps::Stamp;
use crate::walk::recall_walk_multi;

/// The candidate compensations one walk entry carries, in
/// order.
///
/// The set is a function of the class and the status and of
/// nothing else. A missing artifact carries no candidate. A
/// drifted artifact is quarantined or removed and never
/// rebuilt, because rebuilding would overwrite work the stamp
/// never recorded. A current irreversible artifact is
/// quarantined alone.
pub fn candidates_for(rev_class: &PyStr, status: &PyStr) -> Result<Vec<&'static str>> {
    if status == &"missing" {
        return Ok(Vec::new());
    }
    if status == &"drifted" {
        return Ok(vec![CANDIDATES[2], CANDIDATES[1]]);
    }
    if rev_class == &"reversible" {
        return Ok(vec![CANDIDATES[0], CANDIDATES[1]]);
    }
    if rev_class == &"reversible-with-compensation" {
        return Ok(CANDIDATES.to_vec());
    }
    if rev_class == &"irreversible" {
        return Ok(vec![CANDIDATES[2]]);
    }
    Err(Error::Walk(format!(
        "no compensation candidates are defined for the class {rev_class}"
    )))
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlannedCompensation {
    pub name: String,
    pub score: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanEntry {
    pub path: PyStr,
    pub rev_class: PyStr,
    pub status: PyStr,
    pub chain: Vec<PyStr>,
    pub proposed: bool,
    pub compensations: Vec<PlannedCompensation>,
}

/// Place trusted scored candidates first, then the unscored.
///
/// Scored candidates order by score from highest to lowest,
/// and the sort is stable over the canonical order, so equal
/// scores keep their canonical order among themselves.
/// Candidates no trusted score covers follow in their
/// canonical order.
fn order_candidates(
    candidates: &[&'static str],
    scores: &HashMap<&'static str, f64>,
) -> Vec<&'static str> {
    let mut scored: Vec<&'static str> = candidates
        .iter()
        .copied()
        .filter(|name| scores.contains_key(name))
        .collect();
    scored.sort_by(|left, right| {
        scores[right]
            .partial_cmp(&scores[left])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut ordered = scored;
    ordered.extend(
        candidates
            .iter()
            .copied()
            .filter(|name| !scores.contains_key(name)),
    );
    ordered
}

/// The execution plan for one stamped file, in walk order.
///
/// The plan runs the same walk the recall runs, so
/// classification proposals and refusals behave exactly as they
/// behave there. Where a judgment source is given, an artifact
/// carrying two or more candidates is asked about once per
/// candidate, and trusted scores order its candidates under
/// the ranking rule. An artifact carrying fewer than two
/// candidates is never asked about.
pub fn build_plan(
    stamp: &Stamp,
    state: &HashMap<PyStr, PyStr>,
    start: &PyStr,
    player: Option<&dyn JudgmentSource>,
) -> Result<Vec<PlanEntry>> {
    build_plan_multi(stamp, state, std::slice::from_ref(start), player)
}

/// The execution plan for one or more stamped files, in walk
/// order, under BB-42 to BB-47.
///
/// The plan runs the multi-start walk the recall runs, so
/// classification proposals and refusals behave exactly as
/// they behave there, and each artifact of the union is asked
/// its ranking questions once, with the chain field of each
/// request naming the chain the walk prints for the artifact
/// under BB-43.
pub fn build_plan_multi(
    stamp: &Stamp,
    state: &HashMap<PyStr, PyStr>,
    starts: &[PyStr],
    player: Option<&dyn JudgmentSource>,
) -> Result<Vec<PlanEntry>> {
    let entries = recall_walk_multi(stamp, state, starts, player)?;
    let mut planned = Vec::new();
    for entry in &entries {
        let candidates = candidates_for(&entry.rev_class, &entry.status)?;
        let mut scores: HashMap<&'static str, f64> = HashMap::new();
        if let Some(source) = player
            && candidates.len() >= 2
        {
            for candidate in &candidates {
                let request = ranking_request(entry, candidate);
                let answer = check_ranking_answer(source.judge_ranking(&request));
                if trusted_score(answer.as_ref()) {
                    let value = answer
                        .as_ref()
                        .and_then(|held| held.get("score"))
                        .and_then(|score| score.as_f64())
                        .expect("a trusted score carries a number");
                    scores.insert(candidate, if value != 0.0 { value } else { 0.0 });
                }
            }
        }
        let ordered = order_candidates(&candidates, &scores);
        planned.push(PlanEntry {
            path: entry.path.clone(),
            rev_class: entry.rev_class.clone(),
            status: entry.status.clone(),
            chain: entry.chain.clone(),
            proposed: entry.proposed,
            compensations: ordered
                .into_iter()
                .map(|name| PlannedCompensation {
                    name: name.to_string(),
                    score: scores.get(name).copied(),
                })
                .collect(),
        });
    }
    Ok(planned)
}

/// Render a plan as the deterministic document text.
pub fn format_plan(start: &PyStr, entries: &[PlanEntry]) -> String {
    format_plan_multi(std::slice::from_ref(start), entries)
}

/// Render a plan for one or more starts, one header line per
/// start in argument order, under BB-44.
pub fn format_plan_multi(starts: &[PyStr], entries: &[PlanEntry]) -> String {
    let mut lines: Vec<String> = starts.iter().map(|start| format!("plan {start}")).collect();
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
        if entry.status == "missing" {
            lines.push("compensation none required, the artifact is already absent".to_string());
            continue;
        }
        for compensation in &entry.compensations {
            match compensation.score {
                None => lines.push(format!("compensation {}", compensation.name)),
                Some(score) => lines.push(format!(
                    "compensation {} score {}",
                    compensation.name,
                    py_float_repr(score)
                )),
            }
        }
    }
    lines.join("\n") + "\n"
}
