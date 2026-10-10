# 0042: submission and teardown lifetimes

- Date: 2026-10-09
- Authorship: recorded by the commit sign-off
- Covers: retained submissions, backend resource destruction, sampler replacement,
  and concurrent context teardown

## Sources and scope

The changes and fixtures are original project experiments involving synthetic
commands, clears, and readbacks. The relevant public rules are
[semaphore destruction](https://docs.vulkan.org/refpages/latest/refpages/source/vkDestroySemaphore.html),
[device destruction](https://docs.vulkan.org/refpages/latest/refpages/source/vkDestroyDevice.html),
[pool reset](https://docs.vulkan.org/refpages/latest/refpages/source/vkResetCommandPool.html),
[host timeline waits](https://docs.vulkan.org/refpages/latest/refpages/source/vkWaitSemaphores.html),
and [queue submission](https://docs.vulkan.org/refpages/latest/refpages/source/vkQueueSubmit.html).

## Findings

A retained `Submission` kept both a context and its timeline alive, but declared
the context first. Rust drops fields in declaration order. When the submission
was the last context owner, device destruction preceded semaphore destruction.
The concurrent teardown fixture reproduced a live child semaphore at device
destruction under synchronization validation. Declaring the context last
preserves the device until the timeline reference has been released.

Backend fields begin with samplers and graphics caches. Their destruction could
precede the waits performed by image and readback command owners, while deferred
recording or device execution still used those objects. Backend destruction now
retires the queued prefix before field destruction begins. Context destruction
already drains the queue, closes recording inputs, joins recording workers,
joins the submit thread, releases its retained timeline, and destroys the device.

Replacing a sampler previously destroyed the old handle immediately. Replacement
now retires queued work before invalidating descriptors and inserting the new
sampler. A gated recording fixture demonstrates the boundary: replacement
returned while the recording was blocked before the fix, and waits for release
after the fix.

Every worker owns a private pool, buffer, and fence. A worker waits for its
completion receipt before resetting its pool. The submit thread resets the fence
only for the next use and publishes successful completion after fence retirement.
Internal submissions and presentation share one submit owner. No additional
shared-pool or fence-reuse defect was found in these paths.

All command owners now observe the existing posting acknowledgement before
polling or waiting for a timeline value. The acknowledgement is published after
the submit call returns. This makes command reuse and release wait for both the
host submission boundary and device completion. Previously that acknowledgement
was required only for readbacks. Waiting for a future timeline signal is legal;
this change alone does not demonstrate a driver defect.

## Regression fixtures

`contexts_retire_before_cross_thread_teardown` starts four producers. Each creates
and destroys four contexts, alternating direct and worker recording and submitting
24 empty command batches, clears, and callback readbacks per context. Teardown
runs on another thread with work pending. A retained submission keeps the context
alive after its other owners are dropped; releasing that submission must release
the final context. The strict validation messenger checks resource teardown.

`sampler_replacement_retires_queued_users` blocks a recording worker, starts sampler
replacement on another thread, and verifies that replacement cannot finish until
the recording is released and retired.

## Validation and limits

Revision `4b92a51` predates the strict runner, so its complete graphics-feature
suite was run five times with loader-forced GPU instrumentation and one test
thread. All five processes completed without the reported crash. These were
baseline crash checks, not clean passes with strict validation: the layer reported
configuration notices about deprecated settings and forced device capabilities.

The series of twenty passes with the graphics feature includes the new regressions. The
external translator experiments are checked separately with all features enabled.
Nineteen of twenty passes finished cleanly with zero validation messages.
Pass 10 failed when the shader compiler could not open a temporary output file.
It reported no validation messages and no driver crash. The same fixture passed
in the other nineteen runs. No driver crash or validation message occurred
across the series.

2026-10-09 follow-up: removing the compiler's output directory after launch
reproduced the shader compiler failure. The sampling fixture now compiles in
memory. [Note 0043](0043-shader-fixture-storage.md) records the storage regression
and repeated validation.

Formatting and strict Clippy passed for all targets and features. Default tests
passed. The complete suite with all features and the external translator
configured passed under both synchronization and GPU-assisted validation, with
zero skipped tests and zero validation messages.

The original intermittent crash on the submit thread was not reproduced. The two
regressions establish real lifetime defects, and the repeat runs bound the
observed behavior after the fixes. They do not prove that either defect caused
the previously reported driver crashes.
