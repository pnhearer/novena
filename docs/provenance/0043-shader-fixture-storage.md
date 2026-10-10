# 0043: shader fixture storage

- Date: 2026-10-09
- Authorship: recorded by the commit sign-off
- Covers: sampling shader compilation and its scratch storage regression

## Failure and evidence

Pass 10 of the earlier twenty-pass series failed in
`sampler_filters_wraps_borders_and_comparison_sample_exactly` at the shader
compiler status assertion. The output named `sample-0-1-true-505167-6.spv`.
With the machine-specific paths removed, the diagnostic was:

```text
ERROR: Failed to open file: <temporary shader output>
<sampling shader source>
```

Cargo returned 101. The texture test executable reported seven tests passed
and one failed. There were zero validation messages. The sampling shader could
not be compiled, so its dispatch and readback did not run.

Contemporaneous retained records show that the configured scratch root was
absent before and after this failure. The fixture recreated that root before
each compilation, but did not retain ownership of the directory across the
compiler process. Removing a task-owned output directory after the compiler
launched reproduced the same diagnostic and exit 101. This establishes a race
between shader output creation and external scratch removal. The retained
records do not identify the process responsible for the original removal.

## Change and regression

The sampling fixture now embeds its original source and compiles it through
[the in-memory compiler API](https://docs.rs/shaderc/latest/shaderc/).
The compiler is a development dependency. Container CI installs CMake and Git
for its source-build fallback. The fixture keeps the returned module in memory
and pipes its bytes to the existing validator through standard input.
It retains the target environment, macro variants, structural validation, and
exact pixel assertions. There is no sampling shader output file to remove.

`shader_compilation_without_scratch_storage` runs a child test process with
all three temporary-directory variables pointing at an existing regular file.
This isolates the environment from other tests and makes scratch storage
unavailable deterministically. The child compiles and validates two independent
modules, verifies the module magic, and checks that their words match.
Before the change, this regression returned 101 at directory creation with
`AlreadyExists`. After the change, it passes.

These are original synthetic experiments. The affected dispatch did not run.
The sampling path already has a shader-write to host-read barrier and waits for
queue completion before reading or destroying its resources. The fix removes
the fixture's dependency on output directory lifetime. It does not establish a
cause for the earlier driver crashes.

## Validation

The unchanged executable built from revision `e2c7fbb` ran the sampler test alone
in 200 fresh processes per mode, with one test thread. Validation was selected
through `VULKAN_VALIDATION`; the unvalidated processes had that variable removed.
Each iteration retained its raw output and actual exit status. The resulting
behavioral failure rates are:

| Mode | Failures / iterations | Observed failure rate |
| --- | ---: | ---: |
| Synchronization validation | 0/200 | 0.0% |
| GPU-assisted validation | 0/200 | 0.0% |
| Without validation | 0/200 | 0.0% |

Both validated loops reported zero validation messages. A preliminary layer
lookup configuration error did not reach the sampler checks and is excluded
from these rates. The deliberate directory-removal reproduction is also
separate from this unmodified baseline.

The fixed sampler test passed in each mode. Default tests passed. The complete
all-feature suite, including ignored tests and the configured external
translator, passed without validation and under both validation modes, with
zero validation messages and zero skipped tests.

Twenty consecutive full all-feature GPU-assisted passes completed cleanly:
20 passed, 0 failed, 0 validation messages, 0 skipped tests. Every pass used the
strict runner below and retained both its complete log and actual exit status.
Formatting and Clippy with warnings denied passed for default and all-feature
targets.

```sh
python3 scripts/check-validation.py --mode gpu --features all
```

Repeat the command twenty times with the external translator configured to
repeat the full series. For an isolated measurement, build the texture test
executable and run its exact sampler test 200 times per mode, with
`--include-ignored --nocapture --test-threads=1`.

The failure rates describe these observed runs. Directory removal reproduces
the original compiler diagnostic, while unavailable scratch storage gives a
deterministic failing-before regression. The original removing process remains
unidentified. These results do not prove that an earlier driver crash was
caused by fixture storage.
