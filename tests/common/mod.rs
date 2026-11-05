//! Shared helpers for the badbatch test suites.
//!
//! The library suites call the library directly. The command
//! suites run the compiled binary in a fresh temporary
//! directory per case, including runs whose standard stream is
//! a closed pipe, which is the only reliable failed write sink
//! on this machine (BB-37).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn test_dir(name: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("badbatch-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    dir
}

pub fn write(dir: &Path, rel: &str, text: &str) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, text).expect("write file");
    path
}

pub fn write_bytes(dir: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(&path, bytes).expect("write file");
    path
}

pub fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("read file")
}

/// The outcome of one binary run.
pub struct Outcome {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Outcome {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_badbatch")
}

pub fn run_cli(args: &[&str], cwd: &Path) -> Outcome {
    let out = Command::new(binary())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run badbatch");
    Outcome {
        code: out.status.code().unwrap_or(-1),
        stdout: out.stdout,
        stderr: out.stderr,
    }
}

#[cfg(unix)]
pub fn run_cli_bytes(args: &[&[u8]], cwd: &Path) -> Outcome {
    use std::os::unix::ffi::OsStrExt;
    let out = Command::new(binary())
        .args(args.iter().map(|a| std::ffi::OsStr::from_bytes(a)))
        .current_dir(cwd)
        .output()
        .expect("run badbatch");
    Outcome {
        code: out.status.code().unwrap_or(-1),
        stdout: out.stdout,
        stderr: out.stderr,
    }
}

/// Which standard stream is a closed pipe for the run.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeadStream {
    Stdout,
    Stderr,
}

/// Run the binary with the chosen stream attached to a pipe
/// whose read end is closed before the child starts, so every
/// write to that stream fails the way a closed pipe fails.
pub fn run_cli_dead(args: &[&str], cwd: &Path, dead: DeadStream) -> Outcome {
    let (reader, writer) = std::io::pipe().expect("create pipe");
    drop(reader);
    let mut command = Command::new(binary());
    command.args(args).current_dir(cwd);
    match dead {
        DeadStream::Stdout => {
            command.stdout(Stdio::from(writer));
            command.stderr(Stdio::piped());
        }
        DeadStream::Stderr => {
            command.stderr(Stdio::from(writer));
            command.stdout(Stdio::piped());
        }
    }
    let mut child = command.spawn().expect("spawn badbatch");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if dead == DeadStream::Stderr {
        if let Some(mut taken) = child.stdout.take() {
            let _ = taken.read_to_end(&mut stdout);
        }
    } else if let Some(mut taken) = child.stderr.take() {
        let _ = taken.read_to_end(&mut stderr);
    }
    let status = child.wait().expect("wait for badbatch");
    Outcome {
        code: status.code().unwrap_or(-1),
        stdout,
        stderr,
    }
}

/// One case from tests/cli_corpus.json.
pub struct CorpusCase {
    pub name: String,
    pub source: String,
    pub setup: BTreeMap<String, String>,
    pub argv: Vec<String>,
    pub code: i64,
    pub stdout: String,
    pub stderr: String,
}

pub fn load_corpus() -> Vec<CorpusCase> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("cli_corpus.json"),
    )
    .expect("read corpus");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse corpus");
    let cases = value
        .get("cases")
        .and_then(|c| c.as_array())
        .expect("corpus cases");
    cases
        .iter()
        .map(|case| {
            let setup = case
                .get("setup")
                .and_then(|s| s.as_object())
                .map(|map| {
                    map.iter()
                        .map(|(k, v)| (k.clone(), v.as_str().expect("setup content").to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let expect = case.get("expect").expect("expect block");
            CorpusCase {
                name: case
                    .get("name")
                    .and_then(|n| n.as_str())
                    .expect("case name")
                    .to_string(),
                source: case
                    .get("source")
                    .and_then(|n| n.as_str())
                    .expect("case source")
                    .to_string(),
                setup,
                argv: case
                    .get("argv")
                    .and_then(|a| a.as_array())
                    .expect("case argv")
                    .iter()
                    .map(|a| a.as_str().expect("argv text").to_string())
                    .collect(),
                code: expect
                    .get("code")
                    .and_then(|c| c.as_i64())
                    .expect("expect code"),
                stdout: expect
                    .get("stdout")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                stderr: expect
                    .get("stderr")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
            }
        })
        .collect()
}

/// Run one corpus case in a fresh directory and compare the
/// exit code and both streams byte for byte.
pub fn run_corpus_case(case: &CorpusCase) -> Result<(), String> {
    let dir = test_dir("corpus");
    for (rel, content) in &case.setup {
        write(&dir, rel, content);
    }
    let args: Vec<&str> = case.argv.iter().map(|a| a.as_str()).collect();
    let outcome = run_cli(&args, &dir);
    let result = if outcome.code as i64 != case.code {
        Err(format!(
            "exit code {} != expected {} (stderr {:?})",
            outcome.code,
            case.code,
            outcome.stderr_text()
        ))
    } else if outcome.stdout_text() != case.stdout {
        Err(format!(
            "stdout {:?} != expected {:?}",
            outcome.stdout_text(),
            case.stdout
        ))
    } else if outcome.stderr_text() != case.stderr {
        Err(format!(
            "stderr {:?} != expected {:?}",
            outcome.stderr_text(),
            case.stderr
        ))
    } else {
        Ok(())
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}
