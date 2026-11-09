#!/usr/bin/env python3
"""Run Electron baseline suites in a writable copy of the shipping checkout."""

import argparse
import csv
import os
import re
import signal
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("checkout", type=Path, help="Writable Electron snapshot")
parser.add_argument("logs", type=Path, help="Directory for raw logs and results.csv")
parser.add_argument("--suite", action="append", help="Run only the named suite (repeatable)")
args = parser.parse_args()
checkout = args.checkout.resolve()
logs = args.logs.resolve()
logs.mkdir(parents=True, exist_ok=True)

suites = {
    name: ["bun", "run", name]
    for name in (
        "test", "test:main", "test:scripture-replay", "test:gpu-frames",
        "test:syphon", "test:recorder", "test:decklink",
        "test:audience-local", "test:audience-room",
    )
}
for case in sorted((checkout / "tests").glob("*.integration.cjs")):
    suites[case.name] = ["./node_modules/.bin/electron", str(case.relative_to(checkout))]
suites["scripture-grid.bench.cjs"] = [
    "./node_modules/.bin/electron", "tests/scripture-grid.bench.cjs"
]
if args.suite:
    unknown = set(args.suite) - suites.keys()
    if unknown:
        parser.error("Unknown suites: " + ", ".join(sorted(unknown)))
    suites = {name: suites[name] for name in args.suite}

with (logs / "results.csv").open("w", newline="") as results:
    writer = csv.writer(results)
    writer.writerow(["suite", "exit", "passed", "failed", "skipped"])
    for name, command in suites.items():
        log = logs / (name.replace(":", "-") + ".log")
        with log.open("w") as output:
            try:
                process = subprocess.Popen(
                    command, cwd=checkout, stdout=output,
                    stderr=subprocess.STDOUT, start_new_session=True,
                )
                try:
                    status = process.wait(timeout=240)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    process.wait()
                    status = "timeout"
            except OSError as error:
                output.write(str(error) + "\n")
                status = "launch error"

        transcript = log.read_text(errors="replace")
        counts = []
        for label in ("pass", "fail", "skipped"):
            matches = re.findall(rf"^[#ℹ] {label} (\d+)\s*$", transcript, re.M)
            counts.append(sum(map(int, matches)) if matches else "")
        if name.endswith(".integration.cjs") and not any(value != "" for value in counts):
            counts = [int(status == 0), int(status != 0), 0]
        writer.writerow([name, status, *counts])
        results.flush()
        print(name, status, *counts, flush=True)
