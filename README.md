# badbatch

Walks a batch of stamped artifacts and lists everything downstream of the bad one, so a poisoned prompt or a broken model version can be recalled like a product recall.

Badbatch is a command line tool that works from recorded files. The stamp command reads one run record, the recorded text one run left behind about its own work, and prints the stamp for it, naming the agent, the prompt hash, and the model with the digest of every file and artifact the run produced. The recall command walks the stamp graph from one or more stamped files and lists every downstream artifact once, with its reversibility class, its status against a recorded current state, and the chain of derivation that reaches it. The plan command prints the same walk as a plan for a human reviewer, with the candidate compensations each artifact carries. The walk and the plan report only and change nothing, and executing a recall is a separate reviewed act that this tool never performs. Flighttape records the run itself, and badbatch stamps what the run left behind.

[![CI](https://github.com/david-engelmann/badbatch/actions/workflows/ci.yml/badge.svg)](https://github.com/david-engelmann/badbatch/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

```console
$ ./target/debug/badbatch recall examples/stamp.txt examples/current-state.txt src/report.py
recall src/report.py
artifact build/report.html reversible current chain src/report.py -> build/report.html
artifact docs/summary.md reversible-with-compensation missing chain src/report.py -> build/report.html -> docs/summary.md
artifact exports/report.pdf irreversible current chain src/report.py -> build/report.html -> docs/summary.md -> exports/report.pdf
```

## Install

Badbatch is a Rust crate that builds from source with cargo.

```sh
git clone https://github.com/david-engelmann/badbatch.git
cd badbatch
cargo build
```

The binary lands at target/debug/badbatch, and the test suite runs with `cargo test`. The build needs a Unix environment such as Linux, macOS, or WSL on Windows.

## Quickstart

From the repository root, with the committed examples.

```console
$ ./target/debug/badbatch stamp examples/run-record.txt
stamp run-0013
agent writer-3
prompt-hash b41d90
model synth-writer-2
file src/data.csv modified 2e0787a747c1ee8ffb27bb394ec06403d5ec0cb82bce31d88c163d7413c34fee
file src/legacy.py deleted none
file src/notes.txt created 246ab779f0af03540f7be8281a4c88ae9a3160724a92c55ca366b72af10032db
file src/report.py created 132d06a4ef6d4c6d3be527e6c8cfeae8805cfe375b53c795bd4583371c2d6f08
artifact build/chart.png reversible dc777fb3f8889615e748da9cafdbe16fe0fa1d7c0997fb3eb4c4047d9eafaf59 derived-from src/data.csv
artifact build/report.html reversible 12c4808c74ebc0c7ea8139991a7e6b7deef13f0f058bc729212264057c5740f4 derived-from src/report.py
artifact dist/bundle.zip irreversible 41d4b860f08961637c13f18670cb5585d1be4f597f300c7274b39ee7d6eddc18 derived-from src/data.csv
artifact docs/summary.md reversible-with-compensation 684303e5b6aaf5323aabb483b393463bc530a990b1e1d0d706b082d7acb70594 derived-from build/report.html
artifact exports/report.pdf irreversible 492731e3de3cdce6740a08859ff78756a4f4cc00ce16d5e829823bc4dcf5ee4f derived-from docs/summary.md
```

## Usage

### stamp

`badbatch stamp <run-record>` reads one run record and prints its stamp, as the quickstart shows. One line stands per file the run touched, naming the path, the action, and the digest of its recorded content, with the word none in place of a digest for a deleted file, and one line stands per artifact, naming the path, its reversibility class, its digest, and the paths it derives from. A record that breaks the format is refused with one error line naming the source and the defect, and no partial stamp is printed.

### recall

`badbatch recall <stamp> <current-state> <start>...` walks the stamp graph from one or more stamped files at once and prints the union of the artifacts any start reaches, each artifact once, as the block at the top of this page shows for one start. Every artifact line names the artifact, its class, its status against the current state, current or drifted or missing, and its chain, and the chain an artifact carries is the chain the single start walk from the earliest start in argument order that reaches it would print. The walk reports the status it finds and never repairs a drifted artifact and never restores a missing one. With `--fixtures <directory>` the walk also consults recorded judgment fixtures, and a trusted answer may propose a class for an artifact the stamp leaves unclassed, printed as the class word followed by the word proposed and never recorded as the class. Where no trusted answer proposes a class for an unclassed artifact the walk would list, the recall refuses with one error line naming the source and the artifact, and plan refuses the same way, since both run the one walk.

### plan

`badbatch plan <stamp> <current-state> <start>...` runs the same walk and prints a plan for a human reviewer, one header line per start, with the artifact lines of the recall and under each of them the candidate compensations the artifact carries by its class and its status.

```console
$ ./target/debug/badbatch plan examples/stamp.txt examples/current-state.txt src/report.py
plan src/report.py
artifact build/report.html reversible current chain src/report.py -> build/report.html
compensation rebuild-from-parents
compensation remove-artifact
artifact docs/summary.md reversible-with-compensation missing chain src/report.py -> build/report.html -> docs/summary.md
compensation none required, the artifact is already absent
artifact exports/report.pdf irreversible current chain src/report.py -> build/report.html -> docs/summary.md -> exports/report.pdf
compensation quarantine-artifact
```

With `--fixtures <directory>` a trusted recorded ranking may order the candidates an artifact carries, printing the score after the candidate name, and the candidates no trusted score covers keep their canonical order after them. The plan executes nothing and approves nothing, and a ranking is never written back into any file.

Every command reads only the files it is handed and writes nothing to any file. Documents print to the standard output and error lines to the standard error. A file that cannot be read or an input that is malformed refuses with one error line and exit status 2, and a malformed invocation exits with status 2. A line that cannot be printed because its stream is already gone answers with status 120 instead, and a document of 8192 bytes or more that cannot finish printing answers with status 2 when its error line can be printed, and an input past a reader ceiling refuses with status 1, as BB-37 states.

## Status

The tool is complete at the surface its contract describes, with no stubbed or held back part, and the full CI gate runs on every change. Round 10 landed in PR 22, completing the two help pins with the root table assertion and the recursive short derivation. Closed rounds 10 of 100.

## License

MIT. See [LICENSE](LICENSE).
