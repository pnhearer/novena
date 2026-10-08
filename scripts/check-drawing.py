#!/usr/bin/env python3
"""Verify bounded drawing or print the stored vertex-state observations."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FIELDS = {
    "CommandBufferDrawArrays": "x1",
    "CommandBufferDrawArraysInstanced": "x1",
    "CommandBufferDrawElementsBaseVertex": "x1",
    "VertexAttribStateSetFormat": "x1",
    "VertexAttribStateSetStreamIndex": "x1",
    "VertexStreamStateSetStride": "x1",
    "VertexStreamStateSetDivisor": "x1",
    "CommandBufferBindVertexAttribState": "x1",
    "CommandBufferBindVertexStreamState": "x1",
}


def observations():
    reports = {}
    for path in sorted((ROOT / "docs/shapes").glob("*.txt")):
        current = None
        report = {}
        for line in path.read_text().splitlines():
            match = re.match(r"nvn(\w+) calls=(\d+) sampled=(\d+)", line)
            if match:
                name, calls, sampled = match.groups()
                current = name if name in FIELDS else None
                if current:
                    report[current] = {"calls": int(calls), "sampled": int(sampled)}
            elif current and line.startswith("  " + FIELDS[current] + " "):
                report[current]["field"] = line.strip()
        reports[path.name] = report
    print(json.dumps(reports, indent=2, sort_keys=True))


def run(command, log):
    result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True, timeout=300)
    log.write("$ " + " ".join(command) + "\n" + result.stdout + "\n")
    log.flush()
    if result.returncode or any(word in result.stdout for word in
                               ["SKIP", "Validation Error", "VUID-"]):
        print(result.stdout, end="")
        raise RuntimeError(f"check failed with exit code {result.returncode}")
    for line in result.stdout.splitlines():
        if line.startswith(("MATCH", "test result:")):
            print(line)
    print("Passed: " + " ".join(command))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--observations", action="store_true",
                        help="print shape fields without inferring enums")
    args = parser.parse_args()
    if args.observations:
        observations()
        return
    if not os.environ.get("NOVENA_SHADOWBOX_PATH"):
        parser.error("NOVENA_SHADOWBOX_PATH must select a translator crate")
    target = ROOT / "target"
    target.mkdir(exist_ok=True)
    os.environ["CARGO_TARGET_DIR"] = str(target)
    for key, suffix in [("CARGO_HOME", "cargo"), ("XDG_CACHE_HOME", "cache"),
                        ("TMPDIR", "tmp")]:
        path = Path(os.environ.setdefault(key, str(target / "drawing" / suffix)))
        path.mkdir(parents=True, exist_ok=True)
    with (target / "drawing.log").open("w") as log:
        run(["cargo", "fmt", "--all", "--", "--check"], log)
        run(["rustfmt", "--edition", "2021", "--check",
             "crates/novena/tests/shadowbox/drawing.rs"], log)
        run(["cargo", "test", "--workspace", "--locked", "--features", "shadowbox",
             "--test", "shadowbox_drawing", "--", "--ignored", "--nocapture",
             "--test-threads=1"], log)
        run(["cargo", "clippy", "--manifest-path",
             "target/shadowbox-global-memory/first_draw_executes_translated_triangle/Cargo.toml",
             "--target-dir", "target/shadowbox-global-memory/build", "--all-targets",
             "--", "-D", "warnings"], log)
    print("Full output: target/drawing.log")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.TimeoutExpired) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
