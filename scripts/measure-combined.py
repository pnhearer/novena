"""Rerun the original combined timing fixtures and retain numeric samples."""

import argparse
import ast
import csv
import os
from pathlib import Path
import re
import statistics
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def external_directory(variable):
    directory = Path(os.environ[variable]).resolve()
    if directory == ROOT or ROOT in directory.parents:
        raise ValueError(f"{variable} must select storage outside the source tree")
    return directory


def write_csv(path, rows):
    with path.open("w", newline="") as stream:
        writer = csv.DictWriter(stream, lineterminator="\n", fieldnames=list(dict.fromkeys(
            key for row in rows for key in row
        )))
        writer.writeheader()
        writer.writerows(rows)


def fields(output):
    return dict(re.findall(r"(\w+)=([0-9.]+)", output))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--runs", type=int, default=12)
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("runs must be positive")
    target = external_directory("CARGO_TARGET_DIR")
    scratch = external_directory("TMPDIR")
    external_directory("XDG_CACHE_HOME")
    if not os.environ.get("NOVENA_SHADOWBOX_PATH"):
        parser.error("configure the external translator first")
    args.output.mkdir(parents=True, exist_ok=True)
    logs = target / "verification" / "measurements"
    logs.mkdir(parents=True, exist_ok=True)

    def run(label, command, environment=None):
        result = subprocess.run(command, cwd=ROOT, env=environment, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        (logs / f"{label}.log").write_text(result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(f"{label} exited {result.returncode}:\n{result.stdout}{result.stderr}")
        if "SKIP" in result.stdout + result.stderr:
            raise RuntimeError(f"{label} skipped verification")
        return result.stdout

    run("build", ["cargo", "build", "--release", "--workspace", "--all-features",
                  "--examples"])
    pipeline = run("pipeline", ["cargo", "test", "--release", "--all-features",
                   "--test", "shadowbox_drawing", "translated_pipeline_first_use",
                   "--", "--include-ignored", "--nocapture", "--test-threads=1"])
    frames = []
    stages = []
    for trial, first, frame in re.findall(
            r"trial=(\d+) SAMPLES first_ns=(\[[^\n]+?\]) frame_ns=(\[[^\n]+?\])",
            pipeline):
        first, frame = ast.literal_eval(first), ast.literal_eval(frame)
        assert len(first) == len(frame) == 24
        for variant, (first_ns, frame_ns) in enumerate(zip(first, frame), 1):
            frames.append(dict(trial=int(trial), variant=variant,
                               first_ns=first_ns, frame_ns=frame_ns))
    for trial, values in re.findall(r"trial=(\d+) STAGES (\[[^\n]+\])", pipeline):
        for stage, (total, calls, maximum) in enumerate(ast.literal_eval(values)):
            stages.append(dict(trial=int(trial), stage=stage, total_ns=total,
                               calls=calls, max_ns=maximum))
    assert len(frames) == 120 and len(stages) == 85
    assert len({r["trial"] for r in stages}) == 5
    support = re.findall(r"SUPPORT libraries=(\w+) fast=(\w+) parts=(\[[^\n]+?\]) links=(\d+)", pipeline)
    assert len(support) == 5
    print(f"pipeline support: {support}", flush=True)

    examples = target / "release" / "examples"
    rows = []
    variants = [("array", 10000, False), ("ordinary", 10000, False),
                ("double-array", 20000, False), ("indexed", 10000, True)]
    policies = [("serial", "fifo", "serial"), ("fifo", "fifo", "0"),
                ("mailbox", "mailbox", "8")]
    with tempfile.TemporaryDirectory(dir=scratch) as temporary:
        for sample in range(1, args.runs + 1):
            for name, draws, indexed in variants[sample % 4:] + variants[:sample % 4]:
                environment = os.environ.copy()
                environment.pop("NOVENA_DISABLE_PUSH_DESCRIPTORS", None)
                if name == "ordinary":
                    environment["NOVENA_DISABLE_PUSH_DESCRIPTORS"] = "1"
                command = [str(examples / "draw_bench"), "3", str(draws)]
                if indexed:
                    command.append("indexed")
                output = run(f"draw-{sample}-{name}", command, environment)
                row = dict(kind="draw", sample=sample, variant=name, **fields(output))
                assert int(row["descriptor_updates_per_frame"]) == 0
                assert int(row["descriptor_pools_per_frame"]) == 0
                assert int(row["state_binds_per_frame"]) == 100
                rows.append(row)
            output = run(f"record-{sample}", [str(examples / "recording_scale")])
            recording = list(csv.DictReader(output.splitlines()))
            assert [int(row["threads"]) for row in recording] == [1, 2, 4, 8]
            rows.extend(dict(kind="record", sample=sample, variant="viewport-clear",
                             **row) for row in recording)
            for name, mode, tick in policies[sample % 3:] + policies[:sample % 3]:
                output = run(f"present-{sample}-{name}", [
                    str(examples / "textured_triangle"), str(Path(temporary) / "pixels.ppm"),
                    "512", mode, "2", tick])
                row = dict(kind="present", sample=sample, variant=name, **fields(output))
                assert int(row["submitted"]) == 544
                assert int(row["delivered"]) == (68 if name == "mailbox" else 544)
                rows.append(row)
            print(f"completed sample {sample}/{args.runs}", flush=True)

    write_csv(args.output / "0039-combined-samples.csv", rows)
    write_csv(args.output / "0039-frame-samples.csv", frames)
    write_csv(args.output / "0039-stage-samples.csv", stages)
    for kind in ["draw", "record", "present"]:
        selected = [row for row in rows if row["kind"] == kind]
        key = "threads" if kind == "record" else "variant"
        metrics = {
            "draw": ["record_ns_per_command", "submit_cpu_ns_per_draw", "fps"],
            "record": ["cpu_ms_per_frame", "wall_ms_per_frame", "frames_per_second"],
            "present": ["mean_us", "variance_us2"],
        }[kind]
        for group in sorted({row[key] for row in selected}):
            print(kind, group, {metric: statistics.median(
                float(row[metric]) for row in selected if row[key] == group
            ) for metric in metrics})
    for metric in ["first_ns", "frame_ns"]:
        values = sorted(row[metric] / 1e6 for row in frames)
        print(metric, dict(zip(["p50_ms", "p95_ms", "p99_ms", "max_ms"],
                               [values[index - 1] for index in [60, 114, 119, 120]])))


if __name__ == "__main__":
    main()
