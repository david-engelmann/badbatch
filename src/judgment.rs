//! Recorded decision model judgments, played back from
//! fixtures.
//!
//! The roadmap gives badbatch two decision points. At the
//! proposed classification point, where the stamp data bounds
//! no class for an artifact, a judgment may propose one of the
//! three reversibility classes for that artifact. At the
//! compensation ranking point, a judgment may score each
//! candidate compensation for one artifact in an execution
//! plan, rating how completely it restores the prior state.
//! Both judgments are advisory. A proposal is marked as
//! proposed wherever the walk output shows it and never becomes
//! the recorded class, a ranking only orders the options a
//! reviewer sees, and no judgment executes, approves, or
//! triggers a recall.
//!
//! A judgment is recorded as a fixture that pairs one request
//! with one response in the shape a decision model returns,
//! under the name of its point. A choice response carries the
//! chosen label, a confidence, and a probability for every
//! label. A score response carries a score and a confidence.
//! An abstention response carries none of those fields. The
//! fixture player answers a request only from a recorded
//! fixture for the same point whose request matches it exactly,
//! and abstains where no fixture matches. Everything here is
//! offline. The player reads files from one directory and
//! nothing else.

use std::collections::HashMap;
use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::errors::{Error, Result};
use crate::paths::py_path_folded;
use crate::pyjson::{LoadsError, Value, dumps_canonical, loads, py_value_repr};
use crate::pytext::{PyStr, py_repr};
use crate::records::CLASSES;
use crate::stamps::{StampArtifact, is_digest};
use crate::walk::WalkEntry;

pub const CONFIDENCE_THRESHOLD: f64 = 0.6;

pub const JUDGMENT_POINT: &str = "classification";
pub const RANKING_POINT: &str = "compensation-ranking";
pub const CANDIDATES: [&str; 3] = [
    "rebuild-from-parents",
    "remove-artifact",
    "quarantine-artifact",
];

const STATUSES: [&str; 3] = ["current", "drifted", "missing"];
const ENVELOPE_FIELDS: [&str; 4] = ["synthetic", "judgment_point", "request", "response"];
const REQUEST_FIELDS: [&str; 4] = ["artifact", "parents", "digest", "status"];
const RANKING_REQUEST_FIELDS: [&str; 5] = ["artifact", "candidate", "class", "status", "chain"];

fn fail(source: &str, defect: &str) -> Error {
    Error::Judgment(format!("{source} {defect}"))
}

/// A judgment source, answering classification requests
/// through one method and ranking requests through the other,
/// with no answer where it abstains.
pub trait JudgmentSource {
    /// The classification response for one request, or None to
    /// abstain.
    fn judge(&self, request: &Value) -> Option<Value>;
    /// The ranking response for one request, or None to
    /// abstain.
    fn judge_ranking(&self, request: &Value) -> Option<Value>;
}

/// The fields a response of one kind carries at one point.
fn fields_for_kind(point: &str, kind: &str) -> Option<&'static [&'static str]> {
    match (point, kind) {
        (JUDGMENT_POINT, "abstention") => Some(&["kind"]),
        (JUDGMENT_POINT, "choice") => Some(&["kind", "choice", "confidence", "probabilities"]),
        (RANKING_POINT, "abstention") => Some(&["kind"]),
        (RANKING_POINT, "score") => Some(&["kind", "score", "confidence"]),
        _ => None,
    }
}

/// The first of the extra fields, ordered by their printed
/// form, as the precision rule of BB-23 states.
fn first_extra<'a>(pairs: &'a [(PyStr, Value)], allowed: &[&str]) -> Option<&'a PyStr> {
    let mut extras: Vec<&PyStr> = pairs
        .iter()
        .map(|(key, _)| key)
        .filter(|key| !allowed.iter().any(|known| **key == *known))
        .collect();
    extras.sort_by_key(|key| py_repr(key));
    extras.into_iter().next()
}

fn check_unit_interval(source: &str, field: &str, value: Option<&Value>) -> Result<()> {
    let number = value.and_then(|held| held.as_f64());
    let accepted = matches!(value, Some(held) if held.is_number())
        && matches!(number, Some(number) if (0.0..=1.0).contains(&number));
    if !accepted {
        return Err(fail(
            source,
            &format!("field '{field}' is not a number between 0 and 1"),
        ));
    }
    Ok(())
}

fn field_set_matches(pairs: &[(PyStr, Value)], expected: &[&str]) -> bool {
    pairs.len() == expected.len()
        && expected
            .iter()
            .all(|field| pairs.iter().any(|(key, _)| key == field))
}

fn validate_request(source: &str, request: &Value) -> Result<()> {
    let Value::Object(pairs) = request else {
        return Err(fail(source, "field 'request' is not an object"));
    };
    if !field_set_matches(pairs, &REQUEST_FIELDS) {
        return Err(fail(
            source,
            "request does not carry exactly the fields artifact, \
             parents, digest, and status",
        ));
    }
    let artifact = request.get("artifact");
    if !matches!(artifact, Some(Value::Str(text)) if !text.is_empty()) {
        return Err(fail(
            source,
            "request field 'artifact' is not a non-empty string",
        ));
    }
    let parents = request.get("parents");
    let parents_ok = matches!(parents, Some(Value::Array(items))
        if !items.is_empty()
            && items
                .iter()
                .all(|item| matches!(item, Value::Str(text) if !text.is_empty())));
    if !parents_ok {
        return Err(fail(
            source,
            "request field 'parents' is not a non-empty list of strings",
        ));
    }
    let digest = request.get("digest");
    if !matches!(digest, Some(Value::Str(text)) if is_digest(text)) {
        return Err(fail(
            source,
            "request field 'digest' is not sixty-four lowercase \
             hex characters",
        ));
    }
    let status = request.get("status");
    if !matches!(status, Some(Value::Str(text)) if STATUSES.iter().any(|known| text == known)) {
        return Err(fail(
            source,
            "request field 'status' is not current, drifted, or missing",
        ));
    }
    Ok(())
}

fn validate_ranking_request(source: &str, request: &Value) -> Result<()> {
    let Value::Object(pairs) = request else {
        return Err(fail(source, "field 'request' is not an object"));
    };
    if !field_set_matches(pairs, &RANKING_REQUEST_FIELDS) {
        return Err(fail(
            source,
            "request does not carry exactly the fields artifact, \
             candidate, class, status, and chain",
        ));
    }
    let artifact = request.get("artifact");
    if !matches!(artifact, Some(Value::Str(text)) if !text.is_empty()) {
        return Err(fail(
            source,
            "request field 'artifact' is not a non-empty string",
        ));
    }
    let candidate = request.get("candidate");
    if !matches!(candidate, Some(Value::Str(text)) if CANDIDATES.iter().any(|known| text == known))
    {
        return Err(fail(
            source,
            "request field 'candidate' is not one of the three \
             candidate compensations",
        ));
    }
    let class = request.get("class");
    if !matches!(class, Some(Value::Str(text)) if CLASSES.iter().any(|known| text == known)) {
        return Err(fail(
            source,
            "request field 'class' is not one of the three \
             reversibility classes",
        ));
    }
    let status = request.get("status");
    if !matches!(status, Some(Value::Str(text)) if STATUSES.iter().any(|known| text == known)) {
        return Err(fail(
            source,
            "request field 'status' is not current, drifted, or missing",
        ));
    }
    let chain = request.get("chain");
    if !matches!(chain, Some(Value::Str(text)) if !text.is_empty()) {
        return Err(fail(
            source,
            "request field 'chain' is not a non-empty string",
        ));
    }
    Ok(())
}

fn validate_response(source: &str, response: &Value, point: &str) -> Result<()> {
    let Value::Object(pairs) = response else {
        return Err(fail(source, "response is not an object"));
    };
    let kind = response.get("kind");
    if let Some(kind_text) = kind.and_then(|held| held.as_str())
        && let Some(fields) = fields_for_kind(point, &kind_text.to_display())
        && let Some(extra) = first_extra(pairs, fields)
    {
        return Err(fail(
            source,
            &format!(
                "response carries the field {}, which a {} \
                 response does not carry",
                py_repr(extra),
                kind_text
            ),
        ));
    }
    if matches!(kind, Some(Value::Str(text)) if text == &"abstention") {
        return Ok(());
    }
    if point == RANKING_POINT {
        if !matches!(kind, Some(Value::Str(text)) if text == &"score") {
            return Err(fail(
                source,
                &format!(
                    "response kind {} does not answer the \
                     compensation-ranking point, which takes a score \
                     or an abstention",
                    kind.map(py_value_repr)
                        .unwrap_or_else(|| "None".to_string())
                ),
            ));
        }
        check_unit_interval(source, "score", response.get("score"))?;
        check_unit_interval(source, "confidence", response.get("confidence"))?;
        return Ok(());
    }
    if !matches!(kind, Some(Value::Str(text)) if text == &"choice") {
        return Err(fail(
            source,
            &format!(
                "response kind {} does not answer the classification \
                 point, which takes a choice or an abstention",
                kind.map(py_value_repr)
                    .unwrap_or_else(|| "None".to_string())
            ),
        ));
    }
    check_unit_interval(source, "confidence", response.get("confidence"))?;
    let choice = response.get("choice");
    if !matches!(choice, Some(Value::Str(text)) if CLASSES.iter().any(|known| text == known)) {
        return Err(fail(
            source,
            "response field 'choice' is not one of the three \
             reversibility classes",
        ));
    }
    let probabilities = response.get("probabilities");
    let Value::Object(probability_pairs) = probabilities.unwrap_or(&Value::Null) else {
        return Err(fail(
            source,
            "response field 'probabilities' is not an object",
        ));
    };
    if !field_set_matches(probability_pairs, &CLASSES) {
        return Err(fail(
            source,
            "response probabilities do not cover exactly the three \
             reversibility classes",
        ));
    }
    let mut total = 0.0f64;
    for label in CLASSES {
        let held = probabilities.and_then(|object| object.get(label));
        check_unit_interval(source, &format!("probabilities.{label}"), held)?;
        total += held.and_then(|value| value.as_f64()).unwrap_or(0.0);
    }
    if (total - 1.0).abs() > 0.001 {
        return Err(fail(source, "response probabilities do not add up to 1"));
    }
    Ok(())
}

/// One recorded fixture pair, checked against the recorded
/// shape.
///
/// Returns the pair unchanged when it is well formed and
/// raises a judgment refusal naming the source and the defect
/// when it is not.
pub fn validate_fixture(data: &Value, source: &str) -> Result<Value> {
    let Value::Object(pairs) = data else {
        return Err(fail(source, "is not an object"));
    };
    if let Some(extra) = first_extra(pairs, &ENVELOPE_FIELDS) {
        return Err(fail(
            source,
            &format!(
                "carries the field {}, which the fixture envelope \
                 does not carry",
                py_repr(extra)
            ),
        ));
    }
    if !matches!(data.get("synthetic"), Some(Value::Bool(true))) {
        return Err(fail(source, "is not marked synthetic"));
    }
    let point = data.get("judgment_point");
    let point = match point {
        Some(Value::Str(text)) if text == &JUDGMENT_POINT || text == &RANKING_POINT => {
            text.to_display()
        }
        _ => {
            return Err(fail(
                source,
                "field 'judgment_point' names no judgment point this \
                 tool records",
            ));
        }
    };
    let request = data.get("request");
    match request {
        Some(Value::Object(_)) => {}
        _ => return Err(fail(source, "field 'request' is not an object")),
    }
    if point == RANKING_POINT {
        validate_ranking_request(source, request.expect("a checked object"))?;
    } else {
        validate_request(source, request.expect("a checked object"))?;
    }
    if !data.has("response") {
        return Err(fail(source, "carries no response"));
    }
    validate_response(
        source,
        data.get("response").expect("a checked field"),
        &point,
    )?;
    Ok(data.clone())
}

fn request_key(request: &Value, point: &str) -> String {
    let envelope = Value::Object(vec![
        (PyStr::from_str("point"), Value::Str(PyStr::from_str(point))),
        (PyStr::from_str("request"), request.clone()),
    ]);
    let mut out = String::new();
    dumps_canonical(&envelope, &mut out);
    out
}

/// Read and validate one fixture file. The path is the folded
/// joined path the load messages name, and the name is the
/// file name the validation source names.
pub fn load_fixture_file(path_bytes: &[u8], name: &PyStr) -> Result<Value> {
    let display = PyStr::from_surrogateescape(&py_path_folded(path_bytes));
    let bytes = std::fs::read(OsStr::from_bytes(path_bytes))
        .map_err(|_| Error::Judgment(format!("fixture file cannot be read, {display}")))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::Judgment(format!("fixture file is not valid UTF-8, {display}")))?;
    let data = loads(&PyStr::from_str(&text)).map_err(|error| match error {
        LoadsError::Invalid => {
            Error::Judgment(format!("fixture file is not valid JSON, {display}"))
        }
        LoadsError::Uncaught(line) => Error::Uncaught(line),
    })?;
    validate_fixture(&data, &format!("fixture {name}"))
}

/// Answers judgment requests from the recorded fixtures.
///
/// Every JSON file in the directory holds one fixture pair, for
/// either judgment point. Pairs are validated as they load, so
/// a response that changes shape fails here, loudly, and never
/// reaches the walk or the plan. Each point is answered only
/// from its own pairs.
#[derive(Debug)]
pub struct FixturePlayer {
    answers: HashMap<String, Value>,
}

impl FixturePlayer {
    /// Load every fixture in one directory, named as written.
    pub fn load(directory_bytes: &[u8]) -> Result<FixturePlayer> {
        let directory_display = PyStr::from_surrogateescape(directory_bytes);
        // The oracle's path object reads an empty path as the
        // current directory, so the empty string opens there
        // while a refusal still names the path as written.
        let directory = if directory_bytes.is_empty() {
            Path::new(".")
        } else {
            Path::new(OsStr::from_bytes(directory_bytes))
        };
        if !directory.is_dir() {
            return Err(Error::Judgment(format!(
                "fixtures directory cannot be read, {directory_display}"
            )));
        }
        // The oracle's glob suppresses listing failures, so a
        // directory that stands but cannot be listed simply
        // yields no fixtures.
        let mut names: Vec<Vec<u8>> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_bytes = name.as_bytes().to_vec();
                if name_bytes.ends_with(b".json") {
                    names.push(name_bytes);
                }
            }
        }
        // The oracle sorts the joined paths, which share the
        // directory prefix, so the file names decide the order,
        // compared as the strings argument decoding produces.
        names.sort_by(|left, right| {
            PyStr::from_surrogateescape(left).cmp(&PyStr::from_surrogateescape(right))
        });
        let mut answers: HashMap<String, Value> = HashMap::new();
        for name_bytes in &names {
            let mut joined = directory_bytes.to_vec();
            if !joined.is_empty() {
                joined.push(b'/');
            }
            joined.extend_from_slice(name_bytes);
            let name = PyStr::from_surrogateescape(name_bytes);
            let pair = load_fixture_file(&joined, &name)?;
            let key = request_key(
                pair.get("request").expect("a validated request"),
                pair.get("judgment_point")
                    .and_then(|point| point.as_str())
                    .expect("a validated point")
                    .to_display()
                    .as_str(),
            );
            if answers.contains_key(&key) {
                return Err(Error::Judgment(format!(
                    "fixture {name} records the same request as an earlier fixture"
                )));
            }
            answers.insert(
                key,
                pair.get("response").expect("a validated response").clone(),
            );
        }
        Ok(FixturePlayer { answers })
    }

    fn answer(&self, point: &str, request: &Value) -> Option<Value> {
        self.answers.get(&request_key(request, point)).cloned()
    }
}

impl JudgmentSource for FixturePlayer {
    fn judge(&self, request: &Value) -> Option<Value> {
        self.answer(JUDGMENT_POINT, request)
    }

    fn judge_ranking(&self, request: &Value) -> Option<Value> {
        self.answer(RANKING_POINT, request)
    }
}

fn check_answer_at(response: Option<Value>, point: &str) -> Option<Value> {
    let response = response?;
    match validate_response("answer", &response, point) {
        Ok(()) => Some(response),
        Err(_) => None,
    }
}

/// The answer itself when it is well formed for this point,
/// else None.
///
/// A judgment source that is not the fixture player can hand
/// back anything. An answer outside the recorded shape for the
/// classification point is treated as no judgment at all, never
/// trusted and never recorded, so a malformed answer cannot
/// crash the walk or reach a recall result.
pub fn check_answer(response: Option<Value>) -> Option<Value> {
    check_answer_at(response, JUDGMENT_POINT)
}

/// The answer itself when it is well formed for ranking, else
/// None.
///
/// The ranking twin of check_answer. An answer outside the
/// recorded shape for the compensation ranking point is treated
/// as no judgment at all, so a malformed answer cannot reorder
/// a plan.
pub fn check_ranking_answer(response: Option<Value>) -> Option<Value> {
    check_answer_at(response, RANKING_POINT)
}

/// Whether a judgment answer may be used at all.
///
/// An abstention and an answer whose confidence falls below
/// the threshold in the contract both count as no judgment at
/// all.
pub fn trusted(response: Option<&Value>) -> bool {
    let Some(response) = response else {
        return false;
    };
    if !matches!(response.get("kind"), Some(Value::Str(text)) if text == &"choice") {
        return false;
    }
    let confidence = response.get("confidence");
    matches!(confidence, Some(held) if held.is_number())
        && confidence
            .and_then(|held| held.as_f64())
            .is_some_and(|value| value >= CONFIDENCE_THRESHOLD)
}

/// Whether a ranking answer may order a candidate at all.
///
/// A score answer counts only when its score is a number in
/// the unit interval and its confidence reaches the same
/// threshold the classification point trusts. An abstention, a
/// malformed answer, and an answer below the threshold all
/// count as no judgment.
pub fn trusted_score(response: Option<&Value>) -> bool {
    let Some(response) = response else {
        return false;
    };
    if !matches!(response.get("kind"), Some(Value::Str(text)) if text == &"score") {
        return false;
    }
    let score = response.get("score");
    let score_ok = matches!(score, Some(held) if held.is_number())
        && score
            .and_then(|held| held.as_f64())
            .is_some_and(|value| (0.0..=1.0).contains(&value));
    if !score_ok {
        return false;
    }
    let confidence = response.get("confidence");
    matches!(confidence, Some(held) if held.is_number())
        && confidence
            .and_then(|held| held.as_f64())
            .is_some_and(|value| value >= CONFIDENCE_THRESHOLD)
}

/// The classification request the walk asks about one artifact.
///
/// The request is built from the stamp and the walk and from
/// nothing else, naming the artifact, the derivation parents
/// the stamp records for it sorted by path, the digest its
/// stamp line records, and the status the walk computed for it.
pub fn classification_request(artifact: &StampArtifact, status: &str) -> Value {
    let mut parents = artifact.parents.clone();
    parents.sort();
    Value::Object(vec![
        (
            PyStr::from_str("artifact"),
            Value::Str(artifact.path.clone()),
        ),
        (
            PyStr::from_str("parents"),
            Value::Array(parents.into_iter().map(Value::Str).collect()),
        ),
        (
            PyStr::from_str("digest"),
            Value::Str(artifact.digest.clone()),
        ),
        (
            PyStr::from_str("status"),
            Value::Str(PyStr::from_str(status)),
        ),
    ])
}

/// The ranking request the plan asks about one candidate.
///
/// The request is built from the stamp and the walk and from
/// nothing else, naming the artifact, the candidate
/// compensation, the class the walk result carries for the
/// artifact, the status the walk computed for it, and the chain
/// the walk prints for it.
pub fn ranking_request(entry: &WalkEntry, candidate: &str) -> Value {
    let chain = PyStr::join(&entry.chain, &PyStr::from_str(" -> "));
    Value::Object(vec![
        (PyStr::from_str("artifact"), Value::Str(entry.path.clone())),
        (
            PyStr::from_str("candidate"),
            Value::Str(PyStr::from_str(candidate)),
        ),
        (
            PyStr::from_str("class"),
            Value::Str(entry.rev_class.clone()),
        ),
        (PyStr::from_str("status"), Value::Str(entry.status.clone())),
        (PyStr::from_str("chain"), Value::Str(chain)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Python original pinned this against the private
    /// helper directly, so the port pins it here where the
    /// helper is visible. One body under two points keys two
    /// different fixture pairs.
    #[test]
    fn the_request_key_includes_the_point() {
        let body = Value::Object(vec![
            (
                PyStr::from_str("artifact"),
                Value::Str(PyStr::from_str("a")),
            ),
            (
                PyStr::from_str("status"),
                Value::Str(PyStr::from_str("current")),
            ),
        ]);
        assert_ne!(
            request_key(&body, JUDGMENT_POINT),
            request_key(&body, RANKING_POINT)
        );
    }

    /// BB-36. The canonical key escapes non ASCII characters,
    /// and an astral character is written as a surrogate pair,
    /// as the oracle's default separators write it.
    #[test]
    fn the_request_key_writes_an_astral_character_as_a_surrogate_pair() {
        let body = Value::Object(vec![(
            PyStr::from_str("artifact"),
            Value::Str(PyStr(vec![0x61, 0x1F600])),
        )]);
        let key = request_key(&body, JUDGMENT_POINT);
        assert!(key.contains("\\ud83d\\ude00"), "{key}");
    }

    /// BB-34. Two strings that differ only in a surrogate code
    /// point are two different strings, so their keys differ,
    /// and neither answers for the other. A reader that
    /// collapsed a lone surrogate to the replacement character
    /// would key the first and the third of these alike.
    #[test]
    fn request_keys_differ_for_strings_differing_only_in_a_surrogate() {
        let keyed = |point: u32| {
            request_key(
                &Value::Object(vec![(
                    PyStr::from_str("artifact"),
                    Value::Str(PyStr(vec![0x61, point])),
                )]),
                JUDGMENT_POINT,
            )
        };
        assert_ne!(keyed(0xD800), keyed(0xDC00));
        assert_ne!(keyed(0xD800), keyed(0xFFFD));
        assert_ne!(keyed(0xDC00), keyed(0xFFFD));
    }
}
