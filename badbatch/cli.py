"""Command line entry point for badbatch."""

import argparse
import sys
from pathlib import Path

from .errors import BadbatchError
from .judgment import FixturePlayer
from .plan import build_plan, format_plan
from .records import parse_run_record
from .stamps import build_stamp, format_stamp, parse_stamp
from .walk import format_recall, parse_current_state, recall_walk


def _read(path):
    try:
        return Path(path).read_bytes().decode("utf-8")
    except UnicodeDecodeError:
        raise BadbatchError(f"{path} is not readable as utf-8 text")


def main(argv=None):
    parser = argparse.ArgumentParser(
        prog="badbatch",
        description="Stamp what a run left behind and walk the stamp "
                    "graph for a recall.",
    )
    sub = parser.add_subparsers(dest="command", required=True)
    stamp_parser = sub.add_parser(
        "stamp", help="Print the stamp for one recorded run record."
    )
    stamp_parser.add_argument("record")
    recall_parser = sub.add_parser(
        "recall",
        help="Print the recall walk for one stamped file against a "
             "recorded current state.",
    )
    recall_parser.add_argument("stamp")
    recall_parser.add_argument("state")
    recall_parser.add_argument("file")
    recall_parser.add_argument(
        "--fixtures",
        default=None,
        help="Directory of recorded judgment fixtures that may "
             "propose classes for unclassed artifacts.",
    )
    plan_parser = sub.add_parser(
        "plan",
        help="Print the execution plan for one stamped file against "
             "a recorded current state.",
    )
    plan_parser.add_argument("stamp")
    plan_parser.add_argument("state")
    plan_parser.add_argument("file")
    plan_parser.add_argument(
        "--fixtures",
        default=None,
        help="Directory of recorded judgment fixtures that may "
             "propose classes for unclassed artifacts and rank "
             "candidate compensations.",
    )
    args = parser.parse_args(argv)
    try:
        if args.command == "stamp":
            record = parse_run_record(_read(args.record), args.record)
            sys.stdout.write(format_stamp(build_stamp(record)))
        elif args.command == "recall":
            stamp = parse_stamp(_read(args.stamp), args.stamp)
            state = parse_current_state(_read(args.state), args.state)
            player = None
            if args.fixtures is not None:
                player = FixturePlayer(args.fixtures)
            entries = recall_walk(stamp, state, args.file, player)
            sys.stdout.write(format_recall(args.file, entries))
        else:
            stamp = parse_stamp(_read(args.stamp), args.stamp)
            state = parse_current_state(_read(args.state), args.state)
            player = None
            if args.fixtures is not None:
                player = FixturePlayer(args.fixtures)
            planned = build_plan(stamp, state, args.file, player)
            sys.stdout.write(format_plan(args.file, planned))
    except BadbatchError as exc:
        sys.stderr.write(f"error: {exc}\n")
        return 2
    except OSError as exc:
        sys.stderr.write(
            f"error: cannot read {exc.filename}, {exc.strerror}\n"
        )
        return 2
    return 0
