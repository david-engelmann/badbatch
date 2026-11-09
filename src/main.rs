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
    // The root table is the one flag the root parser carries,
    // named outright. The unbuilt command the factory hands
    // back surfaces no root argument at all, and the built
    // command surfaces the generated help argument alone, as
    // the probe of Round 9 recorded, so the table holds the
    // help long because the root declares no other long. The
    // set stands guarded by the pin of BB-57 in the test
    // module below. The subcommand tables are derived from
    // the factory, which does surface theirs.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Derive the root long set from the built command in
    /// the manner subcommand_specs derives the long set of
    /// a subcommand, collecting the long of every argument
    /// the root command carries and completing the set with
    /// the help long where it stands absent.
    fn derived_root_longs() -> Vec<String> {
        let mut command = Cli::command();
        command.build();
        let mut longs: Vec<String> = Vec::new();
        for arg in command.get_arguments() {
            let Some(long) = arg.get_long() else {
                continue;
            };
            let name = format!("--{long}");
            if !longs.contains(&name) {
                longs.push(name);
            }
        }
        if !longs.iter().any(|long| long == "--help") {
            longs.push("--help".to_string());
        }
        longs
    }

    /// BB-57. The hand set the scan reads help tokens
    /// against on the root, the table root_longs returns,
    /// holds the help long alone, and it holds it because
    /// the root command declares no other long. This pin
    /// derives the root long set from the built command and
    /// asserts the derived set is the one help long, so a
    /// long added to the root command, declared as an
    /// argument or raised by an attribute, fails here in
    /// the open instead of leaving the hand set standing
    /// behind the parser in silence.
    ///
    /// BB-61 adds the second assertion, that the table
    /// root_longs returns equals the derived set, so an
    /// edit to the table alone fails here as well.
    #[test]
    fn root_long_set_is_the_one_help_long() {
        assert_eq!(derived_root_longs(), vec!["--help".to_string()]);
        assert_eq!(root_longs(), derived_root_longs());
    }

    /// Derive the short of the help argument of every
    /// command in the tree, walking the built command
    /// recursively, each command first and then its
    /// subcommands in their own order, finding each
    /// argument by its help long in the manner
    /// derived_root_longs collects the longs of the root
    /// command, and standing absent in a command's place
    /// where that command carries no argument with the
    /// help long. BB-62 gives the walk this recursive
    /// form, where the derivation of BB-58 as first
    /// written walked one level deep.
    fn derived_help_shorts() -> Vec<Option<char>> {
        fn help_short(command: &clap::Command) -> Option<char> {
            for arg in command.get_arguments() {
                if arg.get_long() == Some("help") {
                    return arg.get_short();
                }
            }
            None
        }
        fn collect(command: &clap::Command, shorts: &mut Vec<Option<char>>) {
            shorts.push(help_short(command));
            for sub in command.get_subcommands() {
                collect(sub, shorts);
            }
        }
        let mut command = Cli::command();
        command.build();
        let mut shorts = Vec::new();
        collect(&command, &mut shorts);
        shorts
    }

    /// BB-58. The two reads that test the second point of
    /// a token against the code point 0x68, the short help
    /// test of reads_as_positional and the short help
    /// branch of help_read, test that point in every
    /// context the scan walks, on the root and behind a
    /// subcommand alike, and they rest on the help
    /// argument of every command in the tree carrying
    /// the short h. This pin derives the help short of
    /// every command from the built command and asserts
    /// each one is the letter h, so a help short
    /// customized to another letter on any command, or a
    /// command left with no help argument at all, fails
    /// here in the open instead of leaving the two reads
    /// standing behind the parser in silence.
    #[test]
    fn every_help_short_is_the_letter_h() {
        let shorts = derived_help_shorts();
        assert_eq!(shorts.len(), 4);
        assert!(shorts.iter().all(|short| *short == Some('h')));
    }

    /// Build the token stream a scan pin hands the scan.
    fn stream(texts: &[&str]) -> Vec<OsString> {
        texts.iter().map(|text| OsString::from(*text)).collect()
    }

    /// Assert the scan fires the help of the named command,
    /// or the help of the root where the name is absent.
    fn assert_help_fires(tokens: &[&str], command: Option<&str>) {
        match scan(&stream(tokens)) {
            Scan::FireHelp(fired) => assert_eq!(fired.map(|cmd| cmd.name()), command),
            Scan::Assigned(_) => {
                panic!("the scan assigned an invocation where a pin expected help to fire")
            }
            Scan::Run(_) => {
                panic!("the scan handed the stream over where a pin expected help to fire")
            }
        }
    }

    /// Assert the scan hands the invocation to the crate
    /// parser carrying exactly this stream.
    fn assert_handed_over(tokens: &[&str], expected: &[&str]) {
        match scan(&stream(tokens)) {
            Scan::Run(carried) => assert_eq!(carried, stream(expected)),
            Scan::FireHelp(_) => {
                panic!("help fired where a pin expected the stream handed over")
            }
            Scan::Assigned(_) => {
                panic!("the scan assigned an invocation a pin expected handed over")
            }
        }
    }

    /// Assert the scan assigns the invocation to the named
    /// command with these slots and this fixtures value. A
    /// slot written as None in the expectation is a slot
    /// holding a list.
    fn assert_assigned(
        tokens: &[&str],
        command: &str,
        slots: &[Option<&str>],
        fixtures: Option<&str>,
    ) {
        match scan(&stream(tokens)) {
            Scan::Assigned(assigned) => {
                assert_eq!(assigned.cmd.name(), command);
                let carried: Vec<Option<OsString>> = assigned
                    .slots
                    .iter()
                    .map(|slot| match slot {
                        Slot::One(value) => Some(value.clone()),
                        Slot::Listed => None,
                    })
                    .collect();
                let expected: Vec<Option<OsString>> =
                    slots.iter().map(|slot| slot.map(OsString::from)).collect();
                assert_eq!(carried, expected);
                assert_eq!(assigned.fixtures, fixtures.map(OsString::from));
            }
            Scan::FireHelp(_) => panic!("help fired where a pin expected an assignment"),
            Scan::Run(_) => {
                panic!("the scan handed the stream over where a pin expected an assignment")
            }
        }
    }

    /// BB-70, the help family. The exact long in the root
    /// table fires the root help.
    #[test]
    fn scan_help_long_fires_at_the_root() {
        assert_help_fires(&["--help"], None);
    }

    /// BB-70, the help family. An unambiguous abbreviation
    /// of the help long fires, the prefix match of the help
    /// read being the scan's own form of the inference the
    /// parser applies.
    #[test]
    fn scan_help_long_abbreviation_fires_at_the_root() {
        assert_help_fires(&["--hel"], None);
    }

    /// BB-70, the help family. The short alone fires at the
    /// root.
    #[test]
    fn scan_help_short_fires_at_the_root() {
        assert_help_fires(&["-h"], None);
    }

    /// BB-70, the help family. The short leading a cluster
    /// fires, the third point being neither an equals sign
    /// nor a dash.
    #[test]
    fn scan_help_short_cluster_fires_at_the_root() {
        assert_help_fires(&["-hx"], None);
    }

    /// BB-70, the help family. The short carrying an equals
    /// argument is refused on the terms of BB-39, and the
    /// stream handed over carries the replacement the read
    /// builds, the help long with the argument joined.
    #[test]
    fn scan_help_short_with_an_equals_argument_refuses() {
        assert_handed_over(&["-h=x"], &["--help=x"]);
    }

    /// BB-70, the help family. The short carrying a dash led
    /// argument is refused with the replacement built from
    /// the dash on.
    #[test]
    fn scan_help_short_with_a_dash_argument_refuses() {
        assert_handed_over(&["-h-x"], &["--help=-x"]);
    }

    /// BB-70, the help family. The long carrying an equals
    /// argument is refused carrying the token itself, and
    /// the tokens after it pass into the raw stream
    /// untouched, which is how the refusal's block shows
    /// against the no answer reading the token would
    /// otherwise take.
    #[test]
    fn scan_help_long_with_an_equals_argument_refuses() {
        assert_handed_over(&["--help=x", "stamp", "rec"], &["--help=x", "stamp", "rec"]);
    }

    /// BB-70, the help family. The abbreviated long carrying
    /// an equals argument is refused in the same arm at the
    /// root, with the same block standing behind it.
    #[test]
    fn scan_help_abbreviated_long_with_an_equals_argument_refuses() {
        assert_handed_over(&["--hel=x", "stamp", "rec"], &["--hel=x", "stamp", "rec"]);
    }

    /// BB-70, the help family. Behind a subcommand the long
    /// form reads no answer in every shape, because the
    /// subcommand tables are derived from the unbuilt
    /// command, which surfaces no generated help argument,
    /// so no subcommand table holds the help long. The
    /// abbreviated long behind stamp is handed over as
    /// gathered, and the crate parser fires the help on
    /// that stream, which is how the corpus help cases
    /// behind subcommands are answered.
    #[test]
    fn scan_help_abbreviated_long_behind_stamp_reads_no_answer() {
        assert_handed_over(&["stamp", "--h"], &["stamp", "--h"]);
    }

    /// BB-70, the help family. The exact long behind recall
    /// reads no answer on the same table fact, and the
    /// stream is handed over as gathered.
    #[test]
    fn scan_help_long_behind_recall_reads_no_answer() {
        assert_handed_over(&["recall", "--help"], &["recall", "--help"]);
    }

    /// BB-70, the help family. The abbreviated long
    /// carrying an equals argument behind plan reads no
    /// answer in the same arm, and the fall-through stream
    /// shows the gathered order, the token ahead of the
    /// separator and the value behind it.
    #[test]
    fn scan_help_abbreviated_long_with_an_equals_argument_behind_plan_reads_no_answer() {
        assert_handed_over(&["plan", "a", "--hel=x"], &["plan", "--hel=x", "--", "a"]);
    }

    /// BB-70, the help family. The short fires behind plan
    /// and names plan.
    #[test]
    fn scan_help_short_fires_behind_plan() {
        assert_help_fires(&["plan", "-h"], Some("plan"));
    }

    /// BB-70, the help family. Help fires in a deeper
    /// position behind a command, after a value, on the
    /// terms of BB-39.
    #[test]
    fn scan_help_fires_in_a_deeper_position_behind_a_command() {
        assert_help_fires(&["recall", "a", "-h"], Some("recall"));
    }

    /// BB-70, the help family. A consumed fixtures pair does
    /// not block help, which fires behind it.
    #[test]
    fn scan_help_fires_after_a_consumed_fixtures_pair() {
        assert_help_fires(&["recall", "--fixtures=d", "-h"], Some("recall"));
    }

    /// BB-70, the help family. A long that no table entry
    /// begins with reads as no help answer, and the stream
    /// is handed over as gathered.
    #[test]
    fn scan_help_long_matching_no_table_entry_reads_no_answer() {
        assert_handed_over(&["--helpme"], &["--helpme"]);
    }

    /// BB-70, the help family. A long whose name carries a
    /// non ASCII point reads as no help answer, the ASCII
    /// stem read failing before any match is tried.
    #[test]
    fn scan_help_long_with_a_non_ascii_name_reads_no_answer() {
        assert_handed_over(&["--h\u{00e9}"], &["--h\u{00e9}"]);
    }

    /// BB-70, the help family. Help never fires after a
    /// separator, and the short standing after one is a
    /// start value like any other.
    #[test]
    fn scan_help_never_fires_after_a_separator() {
        assert_assigned(
            &["recall", "a", "b", "c", "--", "-h"],
            "recall",
            &[Some("a"), Some("b"), Some("c"), Some("-h")],
            None,
        );
    }

    /// BB-70, the help family. The short help family decides
    /// ahead of the space rule, so the short carrying a
    /// space in one token still fires.
    #[test]
    fn scan_help_short_family_token_carrying_a_space_fires() {
        assert_help_fires(&["stamp", "-h y"], Some("stamp"));
    }

    /// BB-70, the help family. A pending fixtures value
    /// blocks help, the short standing as the value
    /// candidate and reading as no positional there, on the
    /// terms of BB-39.
    #[test]
    fn scan_help_pending_fixtures_value_blocks_help() {
        assert_handed_over(
            &["recall", "--fixtures", "-h"],
            &["recall", "--fixtures", "-h"],
        );
    }

    /// BB-70, the separator family. A separator at the root
    /// blocks the scan, and the stream ends as the raw
    /// stream.
    #[test]
    fn scan_separator_at_the_root_blocks_the_scan() {
        assert_handed_over(&["--", "stamp"], &["--", "stamp"]);
    }

    /// BB-70, the separator family. The first separator in
    /// a segment is discounted, joining the share of the
    /// value before it, and the values assign in order.
    #[test]
    fn scan_separator_between_values_is_discounted() {
        assert_assigned(
            &["recall", "a", "--", "b", "c"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            None,
        );
    }

    /// BB-70, the separator family. A separator leading a
    /// segment joins the first share, which gives it up and
    /// keeps its value.
    #[test]
    fn scan_separator_leading_a_segment_joins_the_first_share() {
        assert_assigned(
            &["recall", "--", "a", "b", "c"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            None,
        );
    }

    /// BB-70, the separator family. Two separators opening
    /// a segment leave the first share holding the
    /// separator itself as a value after the discount.
    #[test]
    fn scan_separator_pair_opens_a_share_holding_the_separator_itself() {
        assert_assigned(
            &["recall", "--", "--", "b", "c"],
            "recall",
            &[Some("--"), Some("b"), Some("c")],
            None,
        );
    }

    /// BB-70, the separator family. A share whose only
    /// token is a later separator gives that separator up
    /// and ends empty, so the state slot holds a list.
    #[test]
    fn scan_separator_pair_leaves_the_state_slot_a_list() {
        assert_assigned(
            &["recall", "stamp.txt", "--", "--", "out/f.html"],
            "recall",
            &[Some("stamp.txt"), None, Some("out/f.html")],
            None,
        );
    }

    /// BB-70, the separator family. The same shape one slot
    /// later leaves the start slot holding a list.
    #[test]
    fn scan_separator_pair_leaves_the_start_slot_a_list() {
        assert_assigned(
            &["recall", "stamp.txt", "state.txt", "--", "--"],
            "recall",
            &[Some("stamp.txt"), Some("state.txt"), None],
            None,
        );
    }

    /// BB-70, the separator family. A trailing separator
    /// after a complete invocation with no flag consumed is
    /// absorbed into the last share and the command runs,
    /// on the terms of BB-46.
    #[test]
    fn scan_trailing_separator_is_absorbed_after_a_complete_invocation() {
        assert_assigned(
            &["recall", "a", "b", "c", "--"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            None,
        );
    }

    /// BB-70, the separator family. A trailing separator
    /// closing a segment that holds nothing else, after a
    /// flag was consumed, refuses the distribution, and the
    /// invocation falls through with the gathered stream, a
    /// separator, and the flattened segment tokens.
    #[test]
    fn scan_trailing_separator_closing_an_empty_segment_after_a_flag_refuses() {
        assert_handed_over(
            &["recall", "a", "b", "c", "--fixtures=dir", "--"],
            &["recall", "--fixtures=dir", "--", "a", "b", "c", "--"],
        );
    }

    /// BB-70, the separator family. A separator closing an
    /// earlier value segment, with a flag pair standing
    /// between values, leaves the distribution standing, on
    /// the terms of BB-46.
    #[test]
    fn scan_separator_closing_a_value_segment_after_a_flag_pair_stands() {
        assert_assigned(
            &["recall", "a", "b", "--fixtures", "dir", "c", "--"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            Some("dir"),
        );
    }

    /// BB-70, the separator family. The fixed shape of
    /// stamp refuses two values, and the fall-through
    /// stream carries the separator and both values.
    #[test]
    fn scan_fixed_shape_overfilled_falls_through() {
        assert_handed_over(&["stamp", "a", "b"], &["stamp", "--", "a", "b"]);
    }

    /// BB-70, the separator family. The variable shape of
    /// recall refuses two slots, the two fixed slots with
    /// no start standing.
    #[test]
    fn scan_variable_shape_underfilled_falls_through() {
        assert_handed_over(&["recall", "a", "b"], &["recall", "--", "a", "b"]);
    }

    /// BB-70, the separator family. Stamp with no value at
    /// all falls through carrying the command word alone,
    /// every segment standing empty.
    #[test]
    fn scan_stamp_with_no_value_falls_through() {
        assert_handed_over(&["stamp"], &["stamp"]);
    }

    /// BB-70, the fixtures family. The joined form after
    /// the values assigns with the fixtures value standing.
    #[test]
    fn scan_fixtures_joined_value_after_the_values() {
        assert_assigned(
            &["recall", "a", "b", "c", "--fixtures=dir"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            Some("dir"),
        );
    }

    /// BB-70, the fixtures family. The separate value
    /// stands as the fixtures value of the assignment.
    #[test]
    fn scan_fixtures_separate_value_stands() {
        assert_assigned(
            &["recall", "--fixtures", "dir", "a", "b", "c"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            Some("dir"),
        );
    }

    /// BB-70, the fixtures family. An unambiguous
    /// abbreviation of the flag's name stands for the flag
    /// in its standing form, on the terms of BB-39.
    #[test]
    fn scan_fixtures_abbreviated_name_standing() {
        assert_assigned(
            &["recall", "--fixt", "dir", "a", "b", "c"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            Some("dir"),
        );
    }

    /// BB-70, the fixtures family. The abbreviation in its
    /// joined form behind plan stands as well.
    #[test]
    fn scan_fixtures_abbreviated_name_joined_behind_plan() {
        assert_assigned(
            &["plan", "--f=dir", "a", "b", "c"],
            "plan",
            &[Some("a"), Some("b"), Some("c")],
            Some("dir"),
        );
    }

    /// BB-70, the fixtures family. A value candidate that
    /// reads as a flag token blocks the scan, and the
    /// stream ends as the raw stream, on the terms of
    /// BB-39.
    #[test]
    fn scan_fixtures_value_reading_as_a_flag_blocks() {
        assert_handed_over(
            &["recall", "--fixtures", "--help"],
            &["recall", "--fixtures", "--help"],
        );
    }

    /// BB-70, the fixtures family. A separator standing
    /// where the value belongs blocks the scan in the same
    /// arm, the pending read of the separator answering not
    /// positional by its first rule.
    #[test]
    fn scan_fixtures_separator_after_the_flag_blocks() {
        assert_handed_over(
            &["recall", "--fixtures", "--", "a"],
            &["recall", "--fixtures", "--", "a"],
        );
    }

    /// BB-70, the fixtures family. The flag standing at the
    /// end with no token after it hands the raw stream
    /// over, the scan still pending.
    #[test]
    fn scan_fixtures_flag_standing_at_the_end_hands_the_raw_stream() {
        assert_handed_over(
            &["recall", "a", "b", "c", "--fixtures"],
            &["recall", "a", "b", "c", "--fixtures"],
        );
    }

    /// BB-70, the fixtures family. A separate value is
    /// joined into the gathered stream at the flag's slot
    /// with an equals sign, and a fall-through hands the
    /// joined form over, the assignment failing here only
    /// for want of slots.
    #[test]
    fn scan_fixtures_pending_value_is_joined_into_the_gathered_stream() {
        assert_handed_over(
            &["recall", "--fixtures", "dir"],
            &["recall", "--fixtures=dir"],
        );
    }

    /// BB-70, the fixtures family. Where the flag stands
    /// more than once its last value stands, on the terms
    /// of BB-38, in the joined repetition.
    #[test]
    fn scan_fixtures_repeated_joined_flag_last_value_stands() {
        assert_assigned(
            &["recall", "--fixtures=one", "--fixtures=two", "a", "b", "c"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            Some("two"),
        );
    }

    /// BB-70, the fixtures family. The last value stands as
    /// well where a separate value is followed by a joined
    /// repetition, the earlier flag token leaving the
    /// gathered stream.
    #[test]
    fn scan_fixtures_separate_then_joined_last_value_stands() {
        assert_assigned(
            &[
                "recall",
                "--fixtures",
                "one",
                "--fixtures=two",
                "a",
                "b",
                "c",
            ],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            Some("two"),
        );
    }

    /// BB-70, the fixtures family. A joined empty value
    /// stands as a fixtures value, distinct from no flag,
    /// the flag's presence being what the command tests on
    /// the terms of BB-38.
    #[test]
    fn scan_fixtures_empty_joined_value_stands_as_a_value() {
        assert_assigned(
            &["recall", "--fixtures=", "a", "b", "c"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            Some(""),
        );
    }

    /// BB-70, the fixtures family. A value that looks like
    /// a negative number is a value in the flag's value
    /// position, on the terms of BB-39.
    #[test]
    fn scan_fixtures_negative_number_stands_as_a_value() {
        assert_assigned(
            &["recall", "--fixtures", "-5", "a", "b", "c"],
            "recall",
            &[Some("a"), Some("b"), Some("c")],
            Some("-5"),
        );
    }

    /// BB-70, the fixtures family. Behind stamp, which
    /// declares no value flag, the flag's name is an
    /// unknown optional, and the stream falls through
    /// dirtied.
    #[test]
    fn scan_fixtures_flag_behind_a_command_without_one_is_an_unknown_optional() {
        assert_handed_over(
            &["stamp", "--fixtures", "x"],
            &["stamp", "--fixtures", "--", "x"],
        );
    }

    /// BB-70, the fixtures family. A stem that extends the
    /// flag's name matches nothing, and the token is an
    /// unknown optional rather than the flag.
    #[test]
    fn scan_fixtures_extended_name_is_not_the_flag() {
        assert_handed_over(
            &["recall", "--fixturesx", "a", "b", "c"],
            &["recall", "--fixturesx", "--", "a", "b", "c"],
        );
    }

    /// BB-70, the recognition family. Stamp assigns its one
    /// slot to its record value.
    #[test]
    fn scan_stamp_assigns_its_one_slot() {
        assert_assigned(&["stamp", "rec"], "stamp", &[Some("rec")], None);
    }

    /// BB-70, the recognition family. A positional at the
    /// root blocks the scan, and every later token passes
    /// into the raw stream untouched.
    #[test]
    fn scan_positional_at_the_root_blocks_the_scan() {
        assert_handed_over(&["word", "stamp", "rec"], &["word", "stamp", "rec"]);
    }

    /// BB-70, the recognition family. A help token after a
    /// root block does not fire, passing into the raw
    /// stream like every token after the block.
    #[test]
    fn scan_tokens_after_a_root_block_pass_untouched() {
        assert_handed_over(&["word", "--help"], &["word", "--help"]);
    }

    /// BB-70, the recognition family. An unknown optional
    /// at the root clears the clean state, and the
    /// fall-through stream carries the optional ahead of
    /// the separator and the gathered values.
    #[test]
    fn scan_unknown_optional_at_the_root_dirties_the_stream() {
        assert_handed_over(
            &["--bogus", "stamp", "rec"],
            &["--bogus", "stamp", "--", "rec"],
        );
    }

    /// BB-70, the recognition family. An unknown optional
    /// behind a command dirties the stream in the same way.
    #[test]
    fn scan_unknown_optional_behind_a_command_dirties_the_stream() {
        assert_handed_over(
            &["stamp", "--bogus", "rec"],
            &["stamp", "--bogus", "--", "rec"],
        );
    }

    /// BB-70, the recognition family. A dirtied stream is
    /// denied the direct run even where the distribution
    /// would fit its values exactly, because an unknown
    /// optional in the gathered stream means the parser
    /// refused the invocation as unrecognized.
    #[test]
    fn scan_unknown_optional_denies_the_direct_run() {
        assert_handed_over(
            &["recall", "--bogus", "a", "b", "c"],
            &["recall", "--bogus", "--", "a", "b", "c"],
        );
    }

    /// BB-70, the recognition family. Subcommand names
    /// never abbreviate, on the terms of BB-39, and the
    /// abbreviation standing at the root reads as a
    /// positional and blocks.
    #[test]
    fn scan_subcommand_names_never_abbreviate() {
        assert_handed_over(&["sta", "rec"], &["sta", "rec"]);
    }

    /// BB-70, the recognition family. An empty stream hands
    /// an empty stream over.
    #[test]
    fn scan_empty_stream_hands_an_empty_stream() {
        assert_handed_over(&[], &[]);
    }

    /// BB-70, the recognition family. With no subcommand
    /// entered, the gathered stream is handed over as it
    /// stands, with no separator appended.
    #[test]
    fn scan_no_subcommand_hands_the_gathered_stream_as_it_stands() {
        assert_handed_over(&["--bogus"], &["--bogus"]);
    }

    /// BB-70, the recognition family. A subcommand's name
    /// behind a command is an ordinary value, the name
    /// match standing only where no command is entered.
    #[test]
    fn scan_subcommand_word_behind_a_command_is_a_value() {
        assert_assigned(&["stamp", "recall"], "stamp", &[Some("recall")], None);
    }

    /// BB-70, the recognition family. A value that looks
    /// like a negative number reads as a positional, on the
    /// terms of BB-39.
    #[test]
    fn scan_negative_number_is_a_positional() {
        assert_assigned(&["stamp", "-5"], "stamp", &[Some("-5")], None);
    }

    /// BB-70, the recognition family. The dotted negative
    /// form with no digit before the point reads as a
    /// positional as well, the digits before the point
    /// standing empty under the pattern.
    #[test]
    fn scan_dotted_negative_with_no_leading_digit_is_a_positional() {
        assert_assigned(&["stamp", "-.5"], "stamp", &[Some("-.5")], None);
    }

    /// BB-70, the recognition family. A trailing point with
    /// no digit after it breaks the negative pattern, and
    /// the token dirties the stream as an unknown optional.
    #[test]
    fn scan_number_with_a_trailing_point_is_not_a_positional() {
        assert_handed_over(&["stamp", "-5."], &["stamp", "-5."]);
    }

    /// BB-70, the recognition family. The negative pattern
    /// matches just before one trailing newline, as the
    /// pattern's end anchor does, so the token carrying it
    /// reads as a positional.
    #[test]
    fn scan_negative_number_before_a_trailing_newline_is_a_positional() {
        assert_assigned(&["stamp", "-5\n"], "stamp", &[Some("-5\n")], None);
    }

    /// BB-70, the recognition family. A lone dash reads as
    /// a positional.
    #[test]
    fn scan_lone_dash_is_a_positional() {
        assert_assigned(&["stamp", "-"], "stamp", &[Some("-")], None);
    }

    /// BB-70, the recognition family. A dash token carrying
    /// a space reads as a positional whatever its prefix.
    #[test]
    fn scan_dash_token_carrying_a_space_is_a_positional() {
        assert_assigned(&["stamp", "-x y"], "stamp", &[Some("-x y")], None);
    }

    /// BB-70, the recognition family. An empty token reads
    /// as a positional and reaches the slot as it stands.
    #[test]
    fn scan_empty_token_is_a_positional() {
        assert_assigned(&["stamp", ""], "stamp", &[Some("")], None);
    }

    /// BB-70, the recognition family. The short help family
    /// decides ahead of the space rule in the pending read
    /// as well, so a fixtures value candidate of that
    /// family carrying a space blocks the scan rather than
    /// standing as a value.
    #[test]
    fn scan_pending_help_family_token_carrying_a_space_blocks() {
        assert_handed_over(
            &["recall", "--fixtures", "-h y", "a"],
            &["recall", "--fixtures", "-h y", "a"],
        );
    }
}
