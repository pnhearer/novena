#!/usr/bin/env python3
"""Run the synthetic pixel checks and, optionally, the native window example."""

import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def run(command, log):
    result = subprocess.run(
        command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, timeout=180,
    )
    log.write("$ " + " ".join(command) + "\n" + result.stdout + "\n")
    log.flush()
    if result.returncode or any(message in result.stdout for message in (
        "Validation Error", "Validation Warning", "VUID-", "SYNC-HAZARD-",
        "Vulkan validation warning:", "Vulkan validation error:",
    )):
        print(result.stdout, end="")
        raise RuntimeError(f"check failed with exit code {result.returncode}")
    print("Passed: " + " ".join(command))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--window", action="store_true", help="also test X11 resize and frame reuse")
    args = parser.parse_args()
    if args.window and not os.environ.get("DISPLAY"):
        parser.error("--window requires an X11 display")
    target = Path(os.environ.get("CARGO_TARGET_DIR", "target")).resolve()
    target.mkdir(exist_ok=True)
    with (target / "presentation.log").open("w") as log:
        run(["cargo", "test", "--locked", "--features", "vulkan", "--test",
             "presentation", "--", "--ignored", "--nocapture"], log)
        if args.window:
            run(["cargo", "build", "--locked", "--features", "vulkan"], log)
            run(["cc", "-std=c11", "-Wall", "-Wextra", "-Werror", "-Iinclude",
                 "examples/present.c", f"-L{target / 'debug'}", "-lnovena", "-lX11",
                 "-lvulkan", "-lm", "-Wl,-rpath,$ORIGIN/debug", "-o", str(target / "present")], log)
            run([str(target / "present"), "--frames", "120", "--resize"], log)
            run([str(target / "present"), "--frames", "120", "--resize", "--unpaced"], log)
    print(f"Full output: {target / 'presentation.log'}")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.TimeoutExpired) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
