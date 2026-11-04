//! Building, printing, and parsing of stamp documents.

use std::collections::HashSet;

use crate::errors::{Error, Result};
use crate::pytext::{PyStr, py_repr};
use crate::records::{
    ACTIONS, CLASSES, PathErrorKind, RunRecord, UNCLASSED, check_path, content_digest,
};

/// Whether a value is sixty four lowercase hex characters.
pub fn is_digest(value: &PyStr) -> bool {
    value.len() == 64
        && value
            .0
            .iter()
            .all(|point| (0x30..=0x39).contains(point) || (0x61..=0x66).contains(point))
}

/// The refusal for a class word outside the three named
/// classes.
///
/// The walk raises this same refusal for an unclassed artifact
/// that no trusted judgment proposes a class for, so the recall
/// refuses that artifact with the exact line the parser gives
/// here.
pub fn class_word_refusal(source: &PyStr, owner: &str, word: &PyStr) -> Error {
    Error::Stamp(format!(
        "{source} is malformed, {owner} names the class {}, which is \
         not one of the three named classes",
        py_repr(word)
    ))
}

#[derive(Clone, Debug, PartialEq)]
pub struct StampFile {
    pub path: PyStr,
    pub action: PyStr,
    pub digest: Option<PyStr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StampArtifact {
    pub path: PyStr,
    pub rev_class: PyStr,
    pub digest: PyStr,
    pub parents: Vec<PyStr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    pub source: PyStr,
    pub run: PyStr,
    pub agent: PyStr,
    pub prompt_hash: PyStr,
    pub model: PyStr,
    pub files: Vec<StampFile>,
    pub artifacts: Vec<StampArtifact>,
}

impl Stamp {
    pub fn file_paths(&self) -> HashSet<&PyStr> {
        self.files.iter().map(|entry| &entry.path).collect()
    }

    pub fn artifact_by_path(&self, path: &PyStr) -> Option<&StampArtifact> {
        self.artifacts.iter().find(|entry| &entry.path == path)
    }
}

/// The stamp for one parsed run record, sorted and digested.
pub fn build_stamp(record: &RunRecord) -> Stamp {
    let mut files: Vec<StampFile> = record
        .files
        .iter()
        .map(|entry| StampFile {
            path: entry.path.clone(),
            action: entry.action.clone(),
            digest: if entry.action == "deleted" {
                None
            } else {
                Some(PyStr::from_str(&content_digest(
                    entry.content.as_deref().unwrap_or(&[]),
                )))
            },
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut artifacts: Vec<StampArtifact> = record
        .artifacts
        .iter()
        .map(|entry| {
            let mut parents = entry.parents.clone();
            parents.sort();
            StampArtifact {
                path: entry.path.clone(),
                rev_class: entry.rev_class.clone(),
                digest: PyStr::from_str(&content_digest(&entry.content)),
                parents,
            }
        })
        .collect();
    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    Stamp {
        source: record.source.clone(),
        run: record.run.clone(),
        agent: record.agent.clone(),
        prompt_hash: record.prompt_hash.clone(),
        model: record.model.clone(),
        files,
        artifacts,
    }
}

/// Render a stamp as the deterministic document text.
pub fn format_stamp(stamp: &Stamp) -> String {
    let mut lines: Vec<String> = vec![
        format!("stamp {}", stamp.run),
        format!("agent {}", stamp.agent),
        format!("prompt-hash {}", stamp.prompt_hash),
        format!("model {}", stamp.model),
    ];
    for entry in &stamp.files {
        let digest = entry
            .digest
            .as_ref()
            .map(|value| value.to_display())
            .unwrap_or_else(|| "none".to_string());
        lines.push(format!("file {} {} {}", entry.path, entry.action, digest));
    }
    for entry in &stamp.artifacts {
        let parents = entry
            .parents
            .iter()
            .map(|parent| parent.to_display())
            .collect::<Vec<String>>()
            .join(", ");
        lines.push(format!(
            "artifact {} {} {} derived-from {}",
            entry.path, entry.rev_class, entry.digest, parents
        ));
    }
    lines.join("\n") + "\n"
}

fn digest_field(
    value: &PyStr,
    source: &PyStr,
    owner: &str,
    allow_none: bool,
) -> Result<Option<PyStr>> {
    if value == &"none" && allow_none {
        return Ok(None);
    }
    if !is_digest(value) {
        return Err(Error::Stamp(format!(
            "{source} is malformed, {owner} carries the digest {}, which \
             is not sixty-four lowercase hex characters",
            py_repr(value)
        )));
    }
    Ok(Some(value.clone()))
}

/// Parse one stamp document or raise a stamp refusal.
pub fn parse_stamp(text: &PyStr, source: &PyStr) -> Result<Stamp> {
    let mut meaningful: Vec<PyStr> = Vec::new();
    for raw_line in text.split_point(b'\n' as u32) {
        let stripped = raw_line.strip();
        if stripped.is_empty() || stripped.starts_with_point(b'#' as u32) {
            continue;
        }
        meaningful.push(stripped);
    }
    let mut cursor = 0usize;
    let take = |cursor: &mut usize| -> Option<PyStr> {
        if *cursor >= meaningful.len() {
            return None;
        }
        let line = meaningful[*cursor].clone();
        *cursor += 1;
        Some(line)
    };
    let first = take(&mut cursor);
    let first_parts = first.as_ref().map(|line| line.split_whitespace());
    let run = match &first_parts {
        Some(parts) if parts.len() == 2 && parts[0] == "stamp" => parts[1].clone(),
        _ => {
            return Err(Error::Stamp(format!(
                "{source} is malformed, it does not open with a stamp line \
                 naming the run"
            )));
        }
    };
    let mut header: Vec<PyStr> = Vec::new();
    for field in ["agent", "prompt-hash", "model"] {
        let line = take(&mut cursor);
        let parts = line.as_ref().map(|line| line.split_whitespace());
        match &parts {
            Some(parts) if parts.len() == 2 && parts[0] == field => {
                header.push(parts[1].clone());
            }
            _ => {
                return Err(Error::Stamp(format!(
                    "{source} is malformed, the stamp is missing its \
                     {field} field"
                )));
            }
        }
    }
    let mut files: Vec<StampFile> = Vec::new();
    let mut artifacts: Vec<StampArtifact> = Vec::new();
    let mut seen: HashSet<PyStr> = HashSet::new();
    while let Some(line) = take(&mut cursor) {
        let parts = line.split_whitespace();
        if parts[0] == "file" && parts.len() == 4 {
            let path = parts[1].clone();
            let action = parts[2].clone();
            let digest_text = parts[3].clone();
            check_path(&path, source, PathErrorKind::Stamp)?;
            if !ACTIONS.iter().any(|known| action == *known) {
                return Err(Error::Stamp(format!(
                    "{source} is malformed, the file line for {path} names \
                     the action {}",
                    py_repr(&action)
                )));
            }
            if seen.contains(&path) {
                return Err(Error::Stamp(format!(
                    "{source} is malformed, the path {path} is stamped twice"
                )));
            }
            seen.insert(path.clone());
            let owner = format!("the file line for {path}");
            let digest = digest_field(&digest_text, source, &owner, action == "deleted")?;
            files.push(StampFile {
                path,
                action,
                digest,
            });
        } else if parts[0] == "artifact" && parts.len() >= 6 && parts[4] == "derived-from" {
            let path = parts[1].clone();
            check_path(&path, source, PathErrorKind::Stamp)?;
            let owner = format!("the artifact line for {path}");
            let rev_class = parts[2].clone();
            if !CLASSES.iter().any(|known| rev_class == *known) && rev_class != UNCLASSED {
                return Err(class_word_refusal(source, &owner, &rev_class));
            }
            if seen.contains(&path) {
                return Err(Error::Stamp(format!(
                    "{source} is malformed, the path {path} is stamped twice"
                )));
            }
            seen.insert(path.clone());
            let digest = digest_field(&parts[3], source, &owner, false)?;
            let joined = PyStr::join(&parts[5..], &PyStr::from_str(" "));
            let parents: Vec<PyStr> = joined
                .split_point(b',' as u32)
                .iter()
                .map(|part| part.strip())
                .collect();
            if parents.iter().any(|parent| parent.is_empty()) {
                return Err(Error::Stamp(format!(
                    "{source} is malformed, {owner} has an empty derivation list"
                )));
            }
            for parent in &parents {
                check_path(parent, source, PathErrorKind::Stamp)?;
            }
            artifacts.push(StampArtifact {
                path,
                rev_class,
                digest: digest.expect("a digest that passed the check"),
                parents,
            });
        } else {
            return Err(Error::Stamp(format!(
                "{source} is malformed, the line {} matches no stamp line form",
                py_repr(&line)
            )));
        }
    }
    for entry in &artifacts {
        for parent in &entry.parents {
            if !seen.contains(parent) {
                return Err(Error::Stamp(format!(
                    "{source} is malformed, the artifact {} derives from \
                     {parent}, which the stamp does not carry",
                    entry.path
                )));
            }
        }
    }
    Ok(Stamp {
        source: source.clone(),
        run,
        agent: header[0].clone(),
        prompt_hash: header[1].clone(),
        model: header[2].clone(),
        files,
        artifacts,
    })
}
