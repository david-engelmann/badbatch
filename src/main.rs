//! Command line entry point, the badbatch binary.
//!
//! Subcommands
//!   stamp   print the stamp for one recorded run record
//!   recall  print the recall walk for one or more stamped
//!           files against a recorded current state
//!   plan    print the execution plan for one or more
//!           stamped files against a recorded current state
//!
//! The argument parsing is clap, adopted under the
//! maintainer's tool choice ruling. Before clap sees the
//! arguments, a scan walks them the way the Python parser's
//! own scan walked them. It fires help where that scan fired
//! it, and where the invocation is clean it runs the command
//! itself on the slots the separator distribution assigns,
//! because the Python parser's separator reading, distributing
//! the values of each segment over the slots that remain when
//! the segment is reached, has no clap counterpart. A segment
//! is a run of values ended by each flag the parser consumes.
//! Within a segment the first separator token is discounted,
//! the separator joins the share of the value just before it,
//! and each share gives up its first separator token, and a
//! share that does not end holding exactly one token holds a
//! list, which surfaces in the uncaught family at the moment
//! that slot is read. An option pair standing between two
//! values changes the segmentation and so can change which
//! slot receives a string and which receives a list, a reading
//! the round's probes corrected against the single run model
//! the earlier fleet ports carried. Where the scan cannot
//! assign the invocation it hands the arguments to clap, as
//! they stand when the scan was blocked and gathered
//! otherwise, since clap refuses the same invocation in its
//! own voice. Help text and malformed invocation error text
//! are clap's own forms. The documents the tool emits and the
//! one error line discipline for its own refusals are
//! unchanged. Badbatch stamps what a run left behind and
//! never records the run itself, and the command line keeps
//! that boundary. It reads recorded documents and prints
//! documents and writes no file.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::process::ExitCode;

use anyhow::Error as AnyError;
use clap::{Args, CommandFactory, Parser, Subcommand};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

use badbatch::errors::Error;
use badbatch::judgment::FixturePlayer;
use badbatch::paths::py_path_folded;
use badbatch::plan::{build_plan_multi, format_plan_multi};
use badbatch::pytext::{PyStr, is_decimal};
use badbatch::records::parse_run_record;
use badbatch::stamps::{build_stamp, format_stamp, parse_stamp};
use badbatch::walk::{format_recall_multi, parse_current_state, recall_walk_multi, start_refusal};

#[derive(Parser)]
#[command(
    name = "badbatch",
    about = "Walks a batch of stamped artifacts and lists everything downstream of the bad one, so a poisoned prompt or a broken model version can be recalled like a product recall.",
    disable_version_flag = true,
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(
        name = "stamp",
        about = "Print the stamp for one recorded run record.",
        infer_long_args = true
    )]
    Stamp(StampArgs),
    #[command(
        name = "recall",
        about = "Print the recall walk for one or more stamped files against a recorded current state.",
        infer_long_args = true
    )]
    Recall(RecallArgs),
    #[command(
        name = "plan",
        about = "Print the execution plan for one or more stamped files against a recorded current state.",
        infer_long_args = true
    )]
    Plan(PlanArgs),
}

#[derive(Args)]
struct StampArgs {
    record: OsString,
}

#[derive(Args)]
struct RecallArgs {
    stamp: OsString,
    state: OsString,
    file: Vec<OsString>,
    #[arg(
        long = "fixtures",
        help = "Directory of recorded judgment fixtures that may propose classes for unclassed artifacts."
    )]
    fixtures: Option<OsString>,
}

#[derive(Args)]
struct PlanArgs {
    stamp: OsString,
    state: OsString,
    file: Vec<OsString>,
    #[arg(
        long = "fixtures",
        help = "Directory of recorded judgment fixtures that may propose classes for unclassed artifacts and rank candidate compensations."
    )]
    fixtures: Option<OsString>,
}

/// The subcommands the command line declares.
#[derive(Clone, Copy, PartialEq)]
enum Cmd {
    Stamp,
    Recall,
    Plan,
}

impl Cmd {
    fn name(self) -> &'static str {
        match self {
            Cmd::Stamp => "stamp",
            Cmd::Recall => "recall",
            Cmd::Plan => "plan",
        }
    }

    fn shape(self) -> SlotShape {
        match self {
            Cmd::Stamp => SlotShape::Fixed(1),
            Cmd::Recall | Cmd::Plan => SlotShape::VariableStart,
        }
    }
}

/// How a command's positional shares are counted by the
/// separator distribution.
///
/// The stamp command fills exactly one slot. The walk
/// commands fill two fixed slots, the stamp and the current
/// state, and every later share stands as one start, under
/// BB-46.
#[derive(Clone, Copy)]
enum SlotShape {
    Fixed(usize),
    VariableStart,
}

/// One subcommand of the command line, with the long flags the
/// derive model declares for it.
///
/// The flag tables of the scan come from clap's own model of
/// the derive structs, read through the command factory, so
/// the derive model stays the one owner of the flag set and
/// this layer restates none of it.
struct SubcommandSpec {
    name: String,
    cmd: Cmd,
    value_flags: Vec<String>,
    longs: Vec<String>,
}

fn subcommand_specs() -> Vec<SubcommandSpec> {
    Cli::command()
        .get_subcommands()
        .map(|sub| {
            let mut value_flags: Vec<String> = Vec::new();
            let mut longs: Vec<String> = Vec::new();
            for arg in sub.get_arguments() {
                let Some(long) = arg.get_long() else {
                    continue;
                };
                let name = format!("--{long}");
                if !longs.contains(&name) {
                    longs.push(name.clone());
                }
                if matches!(arg.get_action(), clap::ArgAction::Set) {
                    value_flags.push(name);
                }
            }
            let cmd = match sub.get_name() {
                "stamp" => Cmd::Stamp,
                "recall" => Cmd::Recall,
                "plan" => Cmd::Plan,
                _ => unreachable!("the derive model declares three subcommands"),
            };
            SubcommandSpec {
                name: sub.get_name().to_string(),
                cmd,
                value_flags,
                longs,
            }
        })
        .collect()
}

fn root_longs() -> Vec<String> {
    // The command factory's model of the root command does not
    // surface the generated help flag, so the root table is the
    // one flag the root parser carries, named outright. The
    // subcommand tables below are derived from the factory,
    // which does surface theirs.
    vec!["--help".to_string()]
}

/// The code points of one argument, decoded the way the
/// oracle's argument decoding decodes them, so the scan's
/// classification reads the same string the Python parser
/// read.
fn points_of(token: &OsStr) -> Vec<u32> {
    PyStr::from_surrogateescape(token.as_bytes()).0
}

/// Whether a body of code points reads as a negative number
/// under the oracle's pattern, one run of decimal digits, or
/// a run of digits, a point, and another run. The digit set
/// is the full Unicode decimal set, as it is in a Python text
/// pattern.
fn is_negative_number(points: &[u32]) -> bool {
    let Some(body) = points.strip_prefix(&[0x2D]) else {
        return false;
    };
    if !body.is_empty() && body.iter().all(|point| is_decimal(*point)) {
        return true;
    }
    match body.iter().position(|point| *point == 0x2E) {
        Some(dot) => {
            let (whole, fraction) = (&body[..dot], &body[dot + 1..]);
            whole.iter().all(|point| is_decimal(*point))
                && !fraction.is_empty()
                && fraction.iter().all(|point| is_decimal(*point))
        }
        None => false,
    }
}

/// Whether the negative number pattern matches the token,
/// where the pattern's end anchor also matches just before
/// one trailing newline, as a Python text pattern's does.
fn negative_number_matches(points: &[u32]) -> bool {
    is_negative_number(points) || points.strip_suffix(&[0x0A]).is_some_and(is_negative_number)
}

/// Whether a token reads as a positional string rather than
/// as an option, the classification the Python parser makes
/// when it meets the token. A bare word and a lone dash are
/// positional. A dash token carrying a space reads as
/// positional whatever its prefix, the one exception being
/// the short help family, whose letter after the dash decides
/// ahead of the space rule. A dash token without a space is
/// positional only when it matches the negative number
/// pattern. Any long token without a space is an option.
fn reads_as_positional(points: &[u32]) -> bool {
    if points == [0x2D, 0x2D] {
        return false;
    }
    if points.first() != Some(&0x2D) || points == [0x2D] {
        return true;
    }
    if points.get(1) == Some(&0x68) {
        return false;
    }
    if points.contains(&0x20) {
        return true;
    }
    if points.starts_with(&[0x2D, 0x2D]) {
        return false;
    }
    negative_number_matches(points)
}

/// How one token reads as a help request.
enum HelpRead {
    /// Not a help token.
    No,
    /// Help fires here.
    Fire,
    /// A help token carrying an explicit argument, which the
    /// Python parser refuses in its own voice, and nothing
    /// later can fire help. The token the stream handed to
    /// clap carries instead, in the form clap refuses there.
    Refuse(OsString),
}

/// The ASCII text of a run of code points, when every point
/// is ASCII, for prefix tests against the ASCII flag names.
fn ascii_text(points: &[u32]) -> Option<String> {
    points
        .iter()
        .map(|point| char::from_u32(*point).filter(|ch| ch.is_ascii()))
        .collect()
}

fn help_read(points: &[u32], token: &OsStr, longs: &[String]) -> HelpRead {
    if points.starts_with(&[0x2D, 0x2D]) {
        let name_points = match points.iter().position(|point| *point == 0x3D) {
            Some(at) => &points[..at],
            None => points,
        };
        if name_points.len() > 2 {
            let Some(name) = ascii_text(name_points) else {
                return HelpRead::No;
            };
            let matches: Vec<&String> = longs
                .iter()
                .filter(|long| long.starts_with(&name))
                .collect();
            if matches.len() == 1 && matches[0] == "--help" {
                if points.contains(&0x3D) {
                    return HelpRead::Refuse(token.to_os_string());
                }
                return HelpRead::Fire;
            }
        }
        return HelpRead::No;
    }
    if points.first() == Some(&0x2D) && points.get(1) == Some(&0x68) {
        return match points.get(2) {
            None => HelpRead::Fire,
            Some(0x3D) => {
                let mut replacement = OsString::from("--help=");
                replacement.push(OsStr::from_bytes(token.as_bytes().get(3..).unwrap_or(&[])));
                HelpRead::Refuse(replacement)
            }
            Some(0x2D) => {
                let mut replacement = OsString::from("--help=");
                replacement.push(OsStr::from_bytes(token.as_bytes().get(2..).unwrap_or(&[])));
                HelpRead::Refuse(replacement)
            }
            Some(_) => HelpRead::Fire,
        };
    }
    HelpRead::No
}

/// How a token reads against the subcommand's one value flag.
enum FlagRead {
    /// Not the value flag.
    No,
    /// The flag with a joined value, the bytes after its first
    /// equals sign.
    Joined(OsString),
    /// The flag standing alone, expecting its value as the
    /// next token.
    Standing,
}

fn flag_read(points: &[u32], token: &OsStr, spec: &SubcommandSpec) -> FlagRead {
    if !points.starts_with(&[0x2D, 0x2D]) || spec.value_flags.is_empty() {
        return FlagRead::No;
    }
    let stem_points = match points.iter().position(|point| *point == 0x3D) {
        Some(at) => &points[..at],
        None => points,
    };
    if stem_points.len() <= 2 {
        return FlagRead::No;
    }
    let Some(stem) = ascii_text(stem_points) else {
        return FlagRead::No;
    };
    let matched: Vec<&String> = spec
        .value_flags
        .iter()
        .filter(|flag| flag.starts_with(&stem))
        .collect();
    if matched.len() != 1 {
        return FlagRead::No;
    }
    match token.as_bytes().iter().position(|byte| *byte == b'=') {
        Some(at) => FlagRead::Joined(OsStr::from_bytes(&token.as_bytes()[at + 1..]).to_os_string()),
        None => FlagRead::Standing,
    }
}

/// One token inside a segment of values.
#[derive(Clone)]
enum SegTok {
    Value(OsString),
    Sep,
}

/// One positional slot after the separator distribution.
enum Slot {
    One(OsString),
    Listed,
}

/// A command the distribution assigned, ready to run.
struct Assigned {
    cmd: Cmd,
    slots: Vec<Slot>,
    fixtures: Option<OsString>,
}

/// The outcome of the argument scan.
enum Scan {
    /// Print the help of the named subcommand parser, or of
    /// the top level parser when the name is absent, at
    /// status 0.
    FireHelp(Option<Cmd>),
    /// Run the command the distribution assigned.
    Assigned(Assigned),
    /// Hand these arguments to clap.
    Run(Vec<OsString>),
}

/// Distribute the segments over the command's slots, in
/// order, each segment filling the next slots that remain
/// when it is reached.
///
/// Within a segment the first separator token is discounted.
/// The first separator joins the share of the value just
/// before it, or the first share when it leads, and every
/// other token, a later separator among them, is a value for
/// the next share in order. Each share then gives up its first
/// separator token, and a share that does not end holding
/// exactly one token holds a list.
fn distribute(segments: &[Vec<SegTok>], shape: SlotShape) -> Option<Vec<Slot>> {
    // A trailing separator that closes a segment holding no
    // positional, once any flag has been consumed, is a usage
    // refusal in the oracle, not a completed invocation: the
    // separator ends the argument string with an empty final
    // group, and the parser's positional match fails on it.
    // Probed against the real parser over the split shapes
    // (a flag pair before it with positionals after parses; the
    // same stream ending one segment earlier refuses).
    if segments.len() > 1
        && let Some(last) = segments.iter().rev().find(|segment| !segment.is_empty())
        && last.len() == 1
        && matches!(last[0], SegTok::Sep)
    {
        return None;
    }
    let mut slots: Vec<Slot> = Vec::new();
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let has_separator = segment.iter().any(|token| matches!(token, SegTok::Sep));
        let arguments = segment.len() - usize::from(has_separator);
        if arguments == 0 {
            continue;
        }
        if let SlotShape::Fixed(count) = shape
            && slots.len() + arguments > count
        {
            return None;
        }
        let first_separator = segment
            .iter()
            .position(|token| matches!(token, SegTok::Sep));
        let mut shares: Vec<Vec<&SegTok>> = vec![Vec::new(); arguments];
        let mut seen = 0usize;
        for (index, token) in segment.iter().enumerate() {
            if Some(index) == first_separator {
                shares[seen.saturating_sub(1)].push(token);
            } else {
                shares[seen].push(token);
                seen += 1;
            }
        }
        for share in &mut shares {
            if let Some(at) = share.iter().position(|token| matches!(token, SegTok::Sep)) {
                share.remove(at);
            }
            match share.len() {
                1 => match share[0] {
                    SegTok::Value(value) => slots.push(Slot::One(value.clone())),
                    SegTok::Sep => slots.push(Slot::One(OsString::from("--"))),
                },
                _ => slots.push(Slot::Listed),
            }
        }
    }
    match shape {
        SlotShape::Fixed(count) => {
            if slots.len() != count {
                return None;
            }
        }
        SlotShape::VariableStart => {
            // The two fixed slots and at least one start must
            // stand, or the invocation is not this command's.
            if slots.len() < 3 {
                return None;
            }
        }
    }
    Some(slots)
}

/// Walk the arguments the way the Python parser's scan walked
/// them, as the module doc describes.
fn scan(tokens: &[OsString]) -> Scan {
    let specs = subcommand_specs();
    let root = root_longs();
    let mut raw: Vec<OsString> = Vec::new();
    let mut out: Vec<OsString> = Vec::new();
    let mut segments: Vec<Vec<SegTok>> = vec![Vec::new()];
    let mut current: Option<usize> = None;
    let mut pending = false;
    let mut fixtures_slot: Option<usize> = None;
    let mut fixtures_value: Option<OsString> = None;
    let mut blocked = false;
    let mut separated = false;
    // Whether nothing but the subcommand word and the consumed
    // fixtures flag has been gathered. An unknown optional in
    // the gathered stream means the Python parser refused the
    // invocation as unrecognized, so the direct run the
    // distribution would otherwise take must not happen.
    let mut clean = true;
    for token in tokens {
        if separated {
            raw.push(token.clone());
            if token.as_bytes() == b"--" {
                segments
                    .last_mut()
                    .expect("a segment always stands")
                    .push(SegTok::Sep);
            } else {
                segments
                    .last_mut()
                    .expect("a segment always stands")
                    .push(SegTok::Value(token.clone()));
            }
            continue;
        }
        if blocked {
            raw.push(token.clone());
            continue;
        }
        let points = points_of(token);
        if pending {
            raw.push(token.clone());
            if reads_as_positional(&points) {
                pending = false;
                fixtures_value = Some(token.clone());
                if let Some(slot) = fixtures_slot {
                    let mut joined = out[slot].clone();
                    joined.push("=");
                    joined.push(token);
                    out[slot] = joined;
                }
            } else {
                blocked = true;
            }
            continue;
        }
        if points == [0x2D, 0x2D] {
            raw.push(token.clone());
            if current.is_some() {
                segments
                    .last_mut()
                    .expect("a segment always stands")
                    .push(SegTok::Sep);
                separated = true;
            } else {
                blocked = true;
            }
            continue;
        }
        let longs: &[String] = match current {
            Some(index) => &specs[index].longs,
            None => &root,
        };
        match help_read(&points, token, longs) {
            HelpRead::Fire => {
                return Scan::FireHelp(current.map(|index| specs[index].cmd));
            }
            HelpRead::Refuse(replacement) => {
                blocked = true;
                raw.push(replacement);
                continue;
            }
            HelpRead::No => {}
        }
        raw.push(token.clone());
        if current.is_none() {
            if let Some(index) = specs
                .iter()
                .position(|spec| spec.name.as_bytes() == token.as_bytes())
            {
                current = Some(index);
                out.push(token.clone());
                continue;
            }
            if reads_as_positional(&points) {
                blocked = true;
            } else {
                clean = false;
                out.push(token.clone());
            }
            continue;
        }
        let spec = &specs[current.expect("a subcommand is selected")];
        match flag_read(&points, token, spec) {
            FlagRead::Joined(value) => {
                fixtures_value = Some(value);
                if let Some(old) = fixtures_slot.take() {
                    out.remove(old);
                }
                out.push(token.clone());
                fixtures_slot = Some(out.len() - 1);
                segments.push(Vec::new());
                continue;
            }
            FlagRead::Standing => {
                if let Some(old) = fixtures_slot.take() {
                    out.remove(old);
                }
                out.push(token.clone());
                fixtures_slot = Some(out.len() - 1);
                pending = true;
                segments.push(Vec::new());
                continue;
            }
            FlagRead::No => {}
        }
        if reads_as_positional(&points) {
            segments
                .last_mut()
                .expect("a segment always stands")
                .push(SegTok::Value(token.clone()));
        } else {
            clean = false;
            out.push(token.clone());
        }
    }
    if blocked || pending {
        return Scan::Run(raw);
    }
    let Some(index) = current else {
        return Scan::Run(out);
    };
    let cmd = specs[index].cmd;
    let assigned = if clean {
        distribute(&segments, cmd.shape())
    } else {
        None
    };
    match assigned {
        Some(slots) => Scan::Assigned(Assigned {
            cmd,
            slots,
            fixtures: fixtures_value,
        }),
        None => {
            if segments.iter().any(|segment| !segment.is_empty()) {
                out.push(OsString::from("--"));
                for segment in &segments {
                    for token in segment {
                        match token {
                            SegTok::Value(value) => out.push(value.clone()),
                            SegTok::Sep => out.push(OsString::from("--")),
                        }
                    }
                }
            }
            Scan::Run(out)
        }
    }
}

/// The size of the Python tool's standard output buffer. A
/// print whose failure surfaces while the output still fits
/// the buffer is the failed flush family, and one that
/// surfaces at or past it is the handler line family.
const STDOUT_BUFFER_BYTES: usize = 8192;

/// The port's own line for a print that failed inside the
/// buffer, standing where the Python tool's interpreter
/// printed its exception ignored noise.
const FAILED_PRINT_LINE: &str = "badbatch could not print its document";

/// The port's own line for a list standing in a slot a command
/// reads as one path, standing where the Python tool printed
/// a traceback for its type error.
const LIST_READ_LINE: &str =
    "badbatch: an argument arrived as a list where a single path was expected";

/// The port's own line for a list standing in the start slot
/// of a walk, standing where the Python tool printed a
/// traceback for its unhashable type error.
const LIST_START_LINE: &str =
    "badbatch: an argument arrived as a list where a single file was expected";

/// A print failure discovered while the output still fit the
/// standard output buffer.
#[derive(Debug)]
struct FailedFlush(String);

impl std::fmt::Display for FailedFlush {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FailedFlush {}

/// The platform error text of a failed read or write, without
/// the suffix the Rust error rendering adds, so the refusal
/// line carries the same words the Python tool's handler
/// reads off its own error.
fn strerror(error: &std::io::Error) -> String {
    let text = error.to_string();
    match error.raw_os_error() {
        Some(code) => text
            .strip_suffix(&format!(" (os error {code})"))
            .map_or(text.clone(), str::to_string),
        None => text,
    }
}

/// Print one document to standard output through one checked
/// write, as the Python tool prints through its buffered
/// write and its exit flush.
///
/// A failure the write itself surfaces at or past the buffer
/// is answered with the handler line the Python command
/// prints for its caught print failure, naming no file. A
/// failure inside the buffer is the failed flush family.
fn print_out(text: &str) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(text.as_bytes())
        .and_then(|()| lock.flush())
        .map_err(|error| {
            if text.len() < STDOUT_BUFFER_BYTES {
                AnyError::new(FailedFlush(FAILED_PRINT_LINE.to_string()))
            } else {
                AnyError::msg(format!("cannot read None, {}", strerror(&error)))
            }
        })
}

/// Write one error line to the error stream through one
/// checked write, and answer with the given status, or with
/// status 120 when the write itself fails.
fn answer_error_line(text: &str, status: u8) -> ExitCode {
    let stderr = std::io::stderr();
    let mut lock = stderr.lock();
    match lock.write_all(text.as_bytes()) {
        Ok(()) => ExitCode::from(status),
        Err(_) => ExitCode::from(120),
    }
}

/// Answer a failed document print.
fn answer_print_failure(failure: AnyError) -> ExitCode {
    if let Some(failed) = failure.downcast_ref::<FailedFlush>() {
        answer_error_line(&format!("{}\n", failed.0), 120)
    } else {
        answer_error_line(&format!("error: {failure}\n"), 2)
    }
}

/// Print a document and answer for its print.
fn print_document(text: &str) -> ExitCode {
    match print_out(text) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => answer_print_failure(failure),
    }
}

/// Answer a library error on the terms the Python command
/// answered it. The refused family prints its line after the
/// error prefix at status 2, and the uncaught family prints
/// the port's own line at status 1.
fn answer_library_error(error: Error) -> ExitCode {
    if error.is_uncaught() {
        answer_error_line(&format!("{}\n", error.message()), 1)
    } else {
        answer_error_line(&format!("error: {}\n", error.message()), 2)
    }
}

/// Answer a clap error the way the command line answers it.
///
/// An error clap prints to standard error, a malformed
/// invocation, is printed there and answered with its own
/// exit code, and it goes through the same handled error
/// stream write as the tool's own lines, so an error stream
/// that cannot take the text answers with status 120. An
/// error clap prints to standard output is the help text, and
/// help output takes the failed write discipline of every
/// document the tool prints, because the Python tool's help
/// goes to the same buffered standard output its documents
/// do.
fn answer_clap_error(error: clap::Error) -> ExitCode {
    if error.use_stderr() {
        let code = error.exit_code();
        let text = error.render().to_string();
        let text = if text.ends_with('\n') {
            text
        } else {
            format!("{text}\n")
        };
        return answer_error_line(&text, if code == 0 { 2 } else { code as u8 });
    }
    let text = error.render().to_string();
    print_document(&text)
}

/// Read one slot's document as text, answering a failure on
/// the spot. The read opens the folded form of the path as
/// written, as the oracle's path object folds before it
/// opens. The cannot read line names that folded form, as
/// the Python handler names it off its exception, and the
/// refusal that a document is not readable as UTF-8 text
/// names the path as written, as does the source every
/// later refusal carries.
fn read_slot(slot: &Slot) -> Result<(PyStr, PyStr), ExitCode> {
    let raw = match slot {
        Slot::Listed => {
            return Err(answer_error_line(&format!("{LIST_READ_LINE}\n"), 1));
        }
        Slot::One(raw) => raw,
    };
    // The fold drops empty and single dot components and
    // reads an empty path as the single dot, so a file
    // spelled with a trailing slash or a trailing dot
    // component opens as the file itself, where a raw open
    // fails with not a directory.
    let folded_bytes = py_path_folded(raw.as_bytes());
    let content = match std::fs::read(OsStr::from_bytes(&folded_bytes)) {
        Ok(content) => content,
        Err(error) => {
            let folded = PyStr::from_surrogateescape(&folded_bytes);
            return Err(answer_error_line(
                &format!("error: cannot read {folded}, {}\n", strerror(&error)),
                2,
            ));
        }
    };
    let source = PyStr::from_surrogateescape(raw.as_bytes());
    match String::from_utf8(content) {
        Ok(text) => Ok((PyStr::from_str(&text), source)),
        Err(_) => Err(answer_error_line(
            &format!("error: {source} is not readable as utf-8 text\n"),
            2,
        )),
    }
}

/// Run one assigned command on its slots, in the read order
/// the Python command reads, stamp, then current state, then
/// the fixtures load, then the start check of the walk.
fn run_assigned(assigned: &Assigned) -> ExitCode {
    if assigned.cmd == Cmd::Stamp {
        let (text, source) = match read_slot(&assigned.slots[0]) {
            Ok(pair) => pair,
            Err(code) => return code,
        };
        return match parse_run_record(&text, &source) {
            Ok(record) => print_document(&format_stamp(&build_stamp(&record))),
            Err(error) => answer_library_error(error),
        };
    }
    let (stamp_text, stamp_source) = match read_slot(&assigned.slots[0]) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let stamp = match parse_stamp(&stamp_text, &stamp_source) {
        Ok(stamp) => stamp,
        Err(error) => return answer_library_error(error),
    };
    let (state_text, state_source) = match read_slot(&assigned.slots[1]) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let state = match parse_current_state(&state_text, &state_source) {
        Ok(state) => state,
        Err(error) => return answer_library_error(error),
    };
    let player = match &assigned.fixtures {
        None => None,
        Some(directory) => match FixturePlayer::load(directory.as_bytes()) {
            Ok(player) => Some(player),
            Err(error) => return answer_library_error(error),
        },
    };
    // The starts are examined in argument order at the start
    // check, after the fixtures load, under BB-45. The first
    // start that cannot stand decides the refusal, a list in
    // the uncaught family's voice and an unknown name in the
    // walk's. A repeated start contributes nothing beyond its
    // first occurrence, under BB-44.
    let mut starts: Vec<PyStr> = Vec::new();
    for slot in &assigned.slots[2..] {
        match slot {
            Slot::Listed => {
                return answer_error_line(&format!("{LIST_START_LINE}\n"), 1);
            }
            Slot::One(raw) => {
                let name = PyStr::from_surrogateescape(raw.as_bytes());
                if !stamp.files.iter().any(|entry| entry.path == name) {
                    return answer_library_error(start_refusal(&name));
                }
                if !starts.contains(&name) {
                    starts.push(name);
                }
            }
        }
    }
    let source_ref = player
        .as_ref()
        .map(|held| held as &dyn badbatch::judgment::JudgmentSource);
    match assigned.cmd {
        Cmd::Recall => match recall_walk_multi(&stamp, &state, &starts, source_ref) {
            Ok(entries) => print_document(&format_recall_multi(&starts, &entries)),
            Err(error) => answer_library_error(error),
        },
        Cmd::Plan => match build_plan_multi(&stamp, &state, &starts, source_ref) {
            Ok(entries) => print_document(&format_plan_multi(&starts, &entries)),
            Err(error) => answer_library_error(error),
        },
        Cmd::Stamp => unreachable!("the stamp command is answered above"),
    }
}

fn run() -> ExitCode {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_else(|| OsString::from("badbatch"));
    let rest: Vec<OsString> = args.collect();
    match scan(&rest) {
        Scan::FireHelp(cmd) => {
            let mut argv = vec![program];
            if let Some(cmd) = cmd {
                argv.push(OsString::from(cmd.name()));
            }
            argv.push(OsString::from("--help"));
            match Cli::try_parse_from(argv) {
                Ok(_) => ExitCode::from(0),
                Err(error) => answer_clap_error(error),
            }
        }
        Scan::Assigned(assigned) => run_assigned(&assigned),
        Scan::Run(tokens) => {
            match Cli::try_parse_from(std::iter::once(program).chain(tokens)) {
                Ok(_) => {
                    // The scan could not assign the invocation,
                    // so the Python parser refused it. Clap's
                    // acceptance of the gathered stream is
                    // answered with a usage refusal, since the
                    // functional outcome is a refusal either way.
                    let usage = Cli::command().render_usage().to_string();
                    answer_error_line(
                        &format!("error: the invocation does not parse\n{usage}\n"),
                        2,
                    )
                }
                Err(error) => answer_clap_error(error),
            }
        }
    }
}

fn main() -> ExitCode {
    // The Python reader reaches documents nested thousands of
    // containers deep before its own ceiling refuses them, and
    // the port's reader and value walks recurse once per
    // container on the deepest path, as the Python tool's do.
    // The binary therefore runs on a worker thread with a
    // stack sized for that depth, so a document at the ceiling
    // is refused or carried on the tool's own terms and never
    // on the stack's.
    match std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(run)
    {
        Ok(handle) => handle.join().unwrap_or(ExitCode::from(2)),
        Err(_) => run(),
    }
}
