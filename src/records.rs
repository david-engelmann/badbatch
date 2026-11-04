//! Parsing of recorded run records, the text one run leaves
//! about its work.

use sha2::{Digest, Sha256};

use crate::errors::{Error, Result};
use crate::pytext::{PyStr, py_repr};

pub const ACTIONS: [&str; 3] = ["created", "modified", "deleted"];
pub const CLASSES: [&str; 3] = [
    "reversible",
    "reversible-with-compensation",
    "irreversible",
];
pub const UNCLASSED: &str = "unclassed";
pub const HEADER_FIELDS: [&str; 4] = ["run", "agent", "prompt-hash", "model"];

/// The digest of recorded content, SHA-256 over one newline
/// per line.
pub fn content_digest(lines: &[PyStr]) -> String {
    let mut digest = Sha256::new();
    for line in lines {
        digest.update(line.to_utf8_bytes());
        digest.update(b"\n");
    }
    format!("{:x}", digest.finalize())
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    pub path: PyStr,
    pub action: PyStr,
    pub content: Option<Vec<PyStr>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactEntry {
    pub path: PyStr,
    pub parents: Vec<PyStr>,
    pub rev_class: PyStr,
    pub content: Vec<PyStr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunRecord {
    pub source: PyStr,
    pub run: PyStr,
    pub agent: PyStr,
    pub prompt_hash: PyStr,
    pub model: PyStr,
    pub files: Vec<FileEntry>,
    pub artifacts: Vec<ArtifactEntry>,
}

/// Which refusal class a path defect raises, records by
/// default, stamps and states under their own classes.
#[derive(Clone, Copy, PartialEq)]
pub enum PathErrorKind {
    Record,
    Stamp,
    State,
}

/// Refuse a path the format cannot carry, naming the defect.
pub fn check_path(path: &PyStr, source: &PyStr, kind: PathErrorKind) -> Result<()> {
    let segments = path.split_point(b'/' as u32);
    let refused = path.is_empty()
        || path.starts_with_point(b'/' as u32)
        || segments.iter().any(|segment| segment == &"..");
    if refused {
        let message = format!(
            "{source} is malformed, the path {} is absolute, \
             empty, or carries a parent reference",
            py_repr(path)
        );
        return Err(match kind {
            PathErrorKind::Record => Error::Record(message),
            PathErrorKind::Stamp => Error::Stamp(message),
            PathErrorKind::State => Error::State(message),
        });
    }
    Ok(())
}

/// The next meaningful line at or after index, as (index,
/// stripped). A meaningful line is one whose stripped form is
/// neither empty nor a comment.
fn meaningful(raw: &[PyStr], mut index: usize) -> (usize, Option<PyStr>) {
    while index < raw.len() {
        let stripped = raw[index].strip();
        if stripped.is_empty() || stripped.starts_with_point(b'#' as u32) {
            index += 1;
            continue;
        }
        return (index, Some(stripped));
    }
    (index, None)
}

/// Consume the content block that must follow an entry line.
///
/// The block opens at the next meaningful line, which must be
/// the word content, and closes at the first later line that
/// reads exactly end. Every line between is literal content.
fn content_block(
    raw: &[PyStr],
    index: usize,
    source: &PyStr,
    owner: &str,
) -> Result<(Vec<PyStr>, usize)> {
    let (open_index, line) = meaningful(raw, index);
    if !matches!(&line, Some(text) if text == &"content") {
        return Err(Error::Record(format!(
            "{source} is malformed, {owner} is not followed by a content block"
        )));
    }
    let mut lines = Vec::new();
    let mut cursor = open_index + 1;
    while cursor < raw.len() {
        if raw[cursor] == "end" {
            return Ok((lines, cursor + 1));
        }
        lines.push(raw[cursor].clone());
        cursor += 1;
    }
    Err(Error::Record(format!(
        "{source} is malformed, the content block for {owner} reaches \
         the end of the record with no closing end line"
    )))
}

fn parents_from_tokens(tokens: &[PyStr], source: &PyStr, owner: &str) -> Result<Vec<PyStr>> {
    let joined = PyStr::join(tokens, &PyStr::from_str(" "));
    let parts = joined.split_point(b',' as u32);
    let parents: Vec<PyStr> = parts.iter().map(|part| part.strip()).collect();
    if joined.is_empty() || parents.iter().any(|parent| parent.is_empty()) {
        return Err(Error::Record(format!(
            "{source} is malformed, {owner} has an empty derivation list"
        )));
    }
    Ok(parents)
}

/// Parse one recorded run record or raise a record refusal.
pub fn parse_run_record(text: &PyStr, source: &PyStr) -> Result<RunRecord> {
    let raw = text.split_point(b'\n' as u32);
    let mut index = 0usize;
    let mut header: Vec<PyStr> = Vec::new();
    for field in HEADER_FIELDS {
        let (found, line) = meaningful(&raw, index);
        index = found;
        let line = match line {
            None => {
                return Err(Error::Record(format!(
                    "{source} is malformed, the header names no {field}"
                )));
            }
            Some(line) => line,
        };
        let parts = line.split_whitespace();
        if parts.len() != 2 || parts[0] != field {
            return Err(Error::Record(format!(
                "{source} is malformed, the header line {} does not name \
                 the {field} exactly once",
                py_repr(&line)
            )));
        }
        header.push(parts[1].clone());
        index += 1;
    }
    let mut files: Vec<FileEntry> = Vec::new();
    let mut artifacts: Vec<ArtifactEntry> = Vec::new();
    loop {
        let (found, line) = meaningful(&raw, index);
        index = found;
        let line = match line {
            None => break,
            Some(line) => line,
        };
        let parts = line.split_whitespace();
        if parts[0] == "file" && parts.len() == 3 {
            let path = parts[1].clone();
            let action = parts[2].clone();
            check_path(&path, source, PathErrorKind::Record)?;
            if !ACTIONS.iter().any(|known| action == *known) {
                return Err(Error::Record(format!(
                    "{source} is malformed, the file entry for {path} names \
                     the action {}, which is not created, modified, or deleted",
                    py_repr(&action)
                )));
            }
            index += 1;
            if action == "deleted" {
                let (_, peek) = meaningful(&raw, index);
                if matches!(&peek, Some(text) if text == &"content") {
                    return Err(Error::Record(format!(
                        "{source} is malformed, the deleted file entry for \
                         {path} carries a content block"
                    )));
                }
                files.push(FileEntry {
                    path,
                    action,
                    content: None,
                });
            } else {
                let owner = format!("the file entry for {path}");
                let (content, next) = content_block(&raw, index, source, &owner)?;
                index = next;
                files.push(FileEntry {
                    path,
                    action,
                    content: Some(content),
                });
            }
        } else if parts[0] == "artifact" && parts.len() >= 5 && parts[2] == "derived-from" {
            let path = parts[1].clone();
            check_path(&path, source, PathErrorKind::Record)?;
            let owner = format!("the artifact entry for {path}");
            if parts[parts.len() - 2] != "class" {
                return Err(Error::Record(format!(
                    "{source} is malformed, {owner} names no class"
                )));
            }
            let rev_class = parts[parts.len() - 1].clone();
            if !CLASSES.iter().any(|known| rev_class == *known) && rev_class != UNCLASSED {
                return Err(Error::Record(format!(
                    "{source} is malformed, {owner} names the class {}, \
                     which is not one of the three named classes",
                    py_repr(&rev_class)
                )));
            }
            let parents = parents_from_tokens(&parts[3..parts.len() - 2], source, &owner)?;
            for parent in &parents {
                check_path(parent, source, PathErrorKind::Record)?;
            }
            index += 1;
            let (content, next) = content_block(&raw, index, source, &owner)?;
            index = next;
            artifacts.push(ArtifactEntry {
                path,
                parents,
                rev_class,
                content,
            });
        } else {
            return Err(Error::Record(format!(
                "{source} is malformed, the line {} matches no entry form",
                py_repr(&line)
            )));
        }
    }
    check_graph(&files, &artifacts, source)?;
    Ok(RunRecord {
        source: source.clone(),
        run: header[0].clone(),
        agent: header[1].clone(),
        prompt_hash: header[2].clone(),
        model: header[3].clone(),
        files,
        artifacts,
    })
}

fn check_graph(files: &[FileEntry], artifacts: &[ArtifactEntry], source: &PyStr) -> Result<()> {
    let file_paths: Vec<&PyStr> = files.iter().map(|entry| &entry.path).collect();
    if let Some(dup) = file_paths
        .iter()
        .find(|path| file_paths.iter().filter(|held| held == path).count() > 1)
    {
        return Err(Error::Record(format!(
            "{source} is malformed, the file path {dup} is entered twice"
        )));
    }
    let artifact_paths: Vec<&PyStr> = artifacts.iter().map(|entry| &entry.path).collect();
    if let Some(dup) = artifact_paths
        .iter()
        .find(|path| artifact_paths.iter().filter(|held| held == path).count() > 1)
    {
        return Err(Error::Record(format!(
            "{source} is malformed, the artifact path {dup} is entered twice"
        )));
    }
    for entry in artifacts {
        if file_paths.contains(&&entry.path) {
            return Err(Error::Record(format!(
                "{source} is malformed, the artifact path {} is also a file path",
                entry.path
            )));
        }
        for parent in &entry.parents {
            if !file_paths.contains(&parent) && !artifact_paths.contains(&parent) {
                return Err(Error::Record(format!(
                    "{source} is malformed, the artifact {} derives from \
                     {parent}, which the record never declares as a file \
                     or as an artifact",
                    entry.path
                )));
            }
        }
    }
    let artifact_set: std::collections::HashSet<&PyStr> =
        artifact_paths.iter().copied().collect();
    let parents_of: std::collections::HashMap<&PyStr, Vec<&PyStr>> = artifacts
        .iter()
        .map(|entry| {
            (
                &entry.path,
                entry
                    .parents
                    .iter()
                    .filter(|parent| artifact_set.contains(parent))
                    .collect::<Vec<&PyStr>>(),
            )
        })
        .collect();
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Open,
        Done,
    }
    let mut state: std::collections::HashMap<&PyStr, Mark> = std::collections::HashMap::new();
    for entry in artifacts {
        if state.contains_key(&entry.path) {
            continue;
        }
        let mut stack: Vec<(&PyStr, Vec<&PyStr>)> = vec![(&entry.path, vec![&entry.path])];
        while let Some((node, trail)) = stack.last().cloned() {
            if state.get(node) == Some(&Mark::Open) {
                state.insert(node, Mark::Done);
                stack.pop();
                continue;
            }
            state.insert(node, Mark::Open);
            let mut descended = false;
            for parent in &parents_of[node] {
                if state.get(*parent) == Some(&Mark::Open) {
                    let mut chain: Vec<&PyStr> = trail.clone();
                    chain.push(parent);
                    let joined = chain
                        .iter()
                        .map(|part| part.to_display())
                        .collect::<Vec<String>>()
                        .join(" then ");
                    return Err(Error::Record(format!(
                        "{source} is malformed, the artifact {parent} derives \
                         from itself through a chain of artifacts, {joined}"
                    )));
                }
                if !state.contains_key(*parent) {
                    let mut next_trail = trail.clone();
                    next_trail.push(parent);
                    stack.push((parent, next_trail));
                    descended = true;
                }
            }
            if !descended {
                state.insert(node, Mark::Done);
                stack.pop();
            }
        }
    }
    Ok(())
}
