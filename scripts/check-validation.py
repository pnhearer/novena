#!/usr/bin/env python3
"""Run every synthetic test with strict Vulkan validation. Provenance: 0041."""

import argparse
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("sync", "gpu", "all"), default="all")
    parser.add_argument("--features", choices=("vulkan", "all"), default="all")
    args = parser.parse_args()
    if args.features == "all" and not os.environ.get("NOVENA_SHADOWBOX_PATH"):
        parser.error("the translator must be configured for all-feature checks")
    root = Path(__file__).resolve().parents[1]
    output = Path(os.environ["CARGO_TARGET_DIR"])
    if not output.is_absolute():
        output = root / output
    output.mkdir(parents=True, exist_ok=True)
    modes = ("sync", "gpu") if args.mode == "all" else (args.mode,)
    failed = False
    for mode in modes:
        env = os.environ.copy()
        env["VULKAN_VALIDATION"] = mode
        command = [
            "cargo", "test", "--locked", "--workspace", "--no-fail-fast",
            *(["--all-features"] if args.features == "all" else ["--features", "vulkan"]), "--",
            "--include-ignored", "--nocapture", "--test-threads=1",
        ]
        messages = 0
        log_path = output / f"validation-{mode}.log"
        with log_path.open("w") as log:
            with subprocess.Popen(
                command, cwd=root, env=env, stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, text=True,
            ) as process:
                for line in process.stdout:
                    log.write(line)
                    log.flush()
                    if "Vulkan validation warning:" in line or "Vulkan validation error:" in line:
                        messages += 1
                        print(line, end="")
                    elif "test result:" in line or "FAILED" in line:
                        print(line, end="")
                status = process.wait()
        failed |= status != 0 or messages != 0
        print(f"{mode}: exit={status}, validation messages={messages}, log={log_path}")
    return int(failed)


if __name__ == "__main__":
    sys.exit(main())
