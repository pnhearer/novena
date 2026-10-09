# 0034: presentation timing and pacing

- Date: 2026-10-09
- Authorship: recorded by the commit sign-off
- Covers: callback queues, native pacing, timeline retirement, frame statistics
  and the synthetic draw measurement

## Source record

No guest signature, format value or structure layout was inferred. Existing
presentation arguments and texture ownership come from [objects](../signatures/0003-objects.md),
[command buffers](../signatures/0002-command-buffer.md) and
[presentation](0026-presentation.md). The original draw scene uses the host
contracts recorded in [textured drawing](0030-textured-blended-drawing.md).

New synchronization and native presentation facts come from public Vulkan
specifications:

- [Timeline submission values](https://docs.vulkan.org/refpages/latest/refpages/source/VkTimelineSemaphoreSubmitInfo.html)
- [Host timeline waits](https://docs.vulkan.org/refpages/latest/refpages/source/vkWaitSemaphores.html)
- [Timeline counter queries](https://docs.vulkan.org/refpages/latest/refpages/source/vkGetSemaphoreCounterValue.html)
- [FIFO and mailbox semantics](https://docs.vulkan.org/refpages/latest/refpages/source/VkPresentModeKHR.html)
- [Presentation retirement fences](https://docs.vulkan.org/refpages/latest/refpages/source/VkSwapchainPresentFenceInfoKHR.html)
- [Presentation maintenance](https://docs.vulkan.org/refpages/latest/refpages/source/VK_EXT_swapchain_maintenance1.html)
- [Presentation errors and queued waits](https://docs.vulkan.org/refpages/latest/refpages/source/vkQueuePresentKHR.html)

All new code and test data are original. No external implementation code,
proprietary documentation or external program data was used.

## Implementation choices

The default is FIFO with two frames in flight. Hosts can select one through
sixteen frames and change between FIFO and mailbox. A policy change drains
pending callbacks before replacing resources. These choices are host policy,
not an inferred guest contract. The C functions are additive and preserve the
host struct layout.

Each callback slot owns an output image, coherent readback buffer and command
resources. A present records the blit and readback, then queues the slot without
waiting for its pixels. FIFO applies backpressure at capacity and delivers the
oldest frame. Mailbox discards pending delivery for the same window, preserving
other windows. A discarded GPU copy still retires before its slot is reused.
Every callback receives the snapshot made at its present request, even if a
later draw changes the source texture.

A nonblocking host poll queries completion and delivers one ready callback.
A draining poll waits on the required timeline values. Queue finish, window
finalization and destruction drain callbacks. There is no presentation worker
or synthetic refresh timer. Hosts poll at their display tick and can supply a
vblank callback. Vblank runs only for frames that are delivered, after the GPU
copy has completed. Callbacks must not reenter the library.

Command slots signal increasing timeline values instead of fences. Reuse
waits for that slot value. CPU arena access uses a separate execution-queue
timeline checkpoint, including work submitted directly through the context.
Mapped bytes are accessed only after preceding queue work completes. No queue
or device idle call remains in the backend. Arena mirroring and synchronous
draw descriptor retirement still require completion waits. This change does
not make the entire draw path asynchronous.

Native output requests FIFO or mailbox, with FIFO fallback when mailbox is
unsupported. It bounds outstanding presentation requests and command slots
by the selected capacity. Binary acquire and presentation semaphores remain
necessary for the native presentation interface. Presentation retirement
fences protect swapchain and semaphore destruction. Native setup requires
presentation maintenance and its surface dependencies; it fails on devices
without them. A zero drawable extent skips delivery and has a separate count.

Statistics report submitted, delivered, replaced, skipped and failed counts,
current and peak callback queue depth, last latency, mean latency and population
variance. The implementation uses online mean and squared-deviation updates.
Callback latency starts at guest present handler entry and ends immediately
before callback entry. It includes backpressure, GPU completion and vblank.
Native latency ends at presentation-engine handoff. Native replacement counts
and scanout timing are unavailable. Statistics remain cumulative across policy
changes.

## Latency experiment

The headless callback presenter receives the original 64 by 64 checkerboard
triangle. Every iteration submits a draw, timestamps immediately before the
guest present call and timestamps again on entry to the host callback. Every
delivered image must contain the blue background and both checkerboard colors.
The existing full pixel assertions also run. The callback timestamp precedes
pixel inspection, so that inspection is outside the sample.

The release baseline executable was built from public revision `b894356` with
only measurement instrumentation. It was retained before implementation.
Each run warmed up with 32 presents, then measured 512 presents. Three paired
runs used the same hardware and original scene. The measurements ran without
a display, vblank callback or validation layer. Machine activity was not
isolated, so these samples describe this run rather than a performance bound.

Serial delivery drains immediately after every present and provides the closest
comparison with the synchronous baseline. Default FIFO fills its two-frame
queue and drains the tail at the end. Mailbox drains every eighth present,
measuring only the newest frame at each host tick. It delivers 64 measured
frames and replaces 448 measured frames per run. Warmup adds four deliveries
and 28 replacements. Dropped frames have no latency sample.

All variances below are population variances. Values are rounded to three
decimal places. The timing excludes drawing before each present, but queued
frames include time spent submitting later draws before host delivery.

| Policy | Pair | Delivered samples | Mean, microseconds | Variance, square microseconds |
| --- | ---: | ---: | ---: | ---: |
| Baseline synchronous | 1 | 512 | 283.770 | 39286.255 |
| Timeline, serial drain | 1 | 512 | 210.358 | 79827.628 |
| Default FIFO, two queued | 1 | 512 | 3784.704 | 940751.443 |
| Mailbox, tick every eight | 1 | 64 | 136.817 | 1437.100 |
| Baseline synchronous | 2 | 512 | 284.064 | 53973.625 |
| Timeline, serial drain | 2 | 512 | 137.339 | 1593.468 |
| Default FIFO, two queued | 2 | 512 | 2080.978 | 765614.250 |
| Mailbox, tick every eight | 2 | 64 | 154.430 | 1834.925 |
| Baseline synchronous | 3 | 512 | 283.914 | 26633.786 |
| Timeline, serial drain | 3 | 512 | 142.425 | 1543.064 |
| Default FIFO, two queued | 3 | 512 | 3965.616 | 1049268.238 |
| Mailbox, tick every eight | 3 | 64 | 233.810 | 51627.691 |

The median run mean fell from 283.914 to 142.425 microseconds with serial
polling. The median run variance fell from 39286.255 to 1593.468 square
microseconds. Default FIFO has greater latency and variance because it retains
two snapshots while the next draws run. Mailbox avoids that backlog by dropping
pending frames. These modes measure different delivery policies. The results
do not establish refresh pacing quality or visible display latency.

To repeat the current measurements, build the `textured_triangle` example in
release mode with the `vulkan` feature. Its arguments are an output image,
measured present count, mode, capacity and optional host tick period. Use
`512 fifo 2 serial` for serial delivery, `512 fifo 2` for queued FIFO and
`512 mailbox 2 8` for mailbox. A tick period of zero drains only at batch end.
Choose the output location and build, temporary and driver-cache directories
through the environment. The example reports callback latency moments and
checks delivery counts, drops and queue bounds.

## Verification

Formatting, strict clippy with default and all features, default workspace
tests and all-feature workspace tests passed. The full run passed 150 tests across the workspace and nested checks, including ignored
GPU tests and all external translator checks, with the translator explicitly
configured. No test was ignored or skipped in that run. The C interface smoke check
compiled with warnings denied and exercised policy, polling, statistics and
invalid-capacity handling.

The callback tests prove FIFO order, immutable snapshots, capacities one,
two, four and sixteen, per-window mailbox replacement, vblank counts, policy
changes, queue finish and destruction. The native headless test exercises
both modes, capacities one, two and four, resize, zero extent and teardown.
A gated GPU test leaves a readback unfinished, checks that a nonblocking poll
returns no frame, then signals the gate and checks the exact pixels. All new
pacing tests and the existing presentation pixel tests pass with the validation
layer enabled and no validation errors.

The full validation run also reports a pre-existing point-size diagnostic in
the depth and raster fixture. That test passes its pixel assertions. The same
diagnostic was checked against the unchanged public base revision. This note
makes no claim that the entire existing suite is free of validation diagnostics.
