# 0040: ordered compilation at submission

- Date: 2026-10-09
- Covers: unfinished translation, bounded compiler queues and draw preservation

## Evidence boundary

This is a host correctness policy. It adds no guest enum interpretation, object
layout or command signature. Command shapes and synthetic execution contracts
remain those in [command execution](0037-command-execution.md),
[textured drawing](0030-textured-blended-drawing.md),
[startup shaders](0031-startup-shaders.md) and
[graphics libraries](0037-graphics-libraries.md).

The previous default discarded a draw when translation, a pipeline part or its
link was unfinished. A full compiler queue could also discard it. Retrying in a
later frame cannot recover a draw issued only once to initialize a texture.

## Policy and order

`PendingDrawPolicy::Block` is the default. The submission executor waits in
place for owned translation, bounded queue admission, pipeline parts and the
executable link. It continues the existing command sequence only after the
request is ready. It never replays the draw in a later submission. Background
workers retain compilation and persistence ownership.

Deferred translation retains its captured context and owned bytes. Submission
waits for admission instead of treating a full queue as success. Compute dispatch
also waits for pending translation rather than reporting it as unsupported.
Failed compilation still returns an execution error and remains available for
explicit retry.

`Skip` and `Wait(Duration)` are explicit host choices. Both can discard draws
and cannot guarantee equivalent output. Timed waits accept zero through 16
milliseconds across pipeline parts and linking. Every discarded draw emits a
standard-error diagnostic with its reason. The existing bounded graphics
diagnostic collector also retains skip messages. Reconfiguring the graphics
cache preserves the selected policy.

## Early-success review

Draw early successes outside compilation cover empty indexed geometry,
zero instance or vertex counts, and explicit render conditionals. Compute
uniform binding records update retained state. Copy and clear execution have
no unfinished-compilation success return. Their early successes cover a
zero channel mask, copying an image to itself, absent selected attachments or
disabled writes. Queue continuations after copy and clear follow successful
execution; failures return errors. Resource reuse and an unchanged completion
checkpoint do not discard required work.

## Regression proof

`cold_draw_writes_once_then_later_submission_samples_exact_pixels` starts with
an empty application pipeline cache and one compiler worker with one waiting
slot. A single full-target draw writes red to a texture. A later submission
samples that texture into another target and reads back exact RGBA bytes for
every pixel. Neither draw is retried. The test covers direct shader words,
a gated pending translation and a full translation queue with deferred owned
bytes. The gate remains closed beyond the old maximum wait budget.

The test failed against the original implementation with blue pixels instead
of red. It passed with ordered waiting in all three cases. Driver disk shader
caching was disabled for these runs.

`blocking_admission_preserves_work_when_the_queue_is_full` gates a worker,
fills its waiting slot and proves every required job is admitted and completed.
`explicit_skip_logs_each_unfinished_draw_and_block_restores_execution` proves
that both explicit skip policies log every unfinished draw and that returning
to blocking execution produces the expected pixels.

Formatting, strict default and all-feature Clippy, and complete default and
all-feature test suites pass. The full-feature run includes ignored graphics
and translator checks. The cold pixel regression also passes with whole-pipeline
fallback forced.
