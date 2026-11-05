//! The oracle's path folding, the string form its path type
//! produces for a path as written.
//!
//! Folding drops empty components and single dot components,
//! keeps parent references, keeps a doubled leading slash as
//! the anchor where longer runs of slashes collapse to one,
//! and reads an empty path as the single dot. Every read
//! opens the folded form, and the cannot read line and the
//! fixture file messages name paths in this folded form,
//! while refusals of a document name the path as written.

/// The folded form of a path, as bytes.
pub fn py_path_folded(path: &[u8]) -> Vec<u8> {
    if path.is_empty() {
        return b".".to_vec();
    }
    let slashes = path.iter().take_while(|byte| **byte == b'/').count();
    let anchor: &[u8] = if slashes == 2 {
        b"//"
    } else if slashes >= 1 {
        b"/"
    } else {
        b""
    };
    let mut parts: Vec<&[u8]> = Vec::new();
    for part in path[slashes..].split(|byte| *byte == b'/') {
        if part.is_empty() || part == b"." {
            continue;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return if anchor.is_empty() {
            b".".to_vec()
        } else {
            anchor.to_vec()
        };
    }
    let mut out = anchor.to_vec();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push(b'/');
        }
        out.extend_from_slice(part);
    }
    out
}
