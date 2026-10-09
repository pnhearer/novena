# 0037: integrated recording, drawing and presentation

- Date: 2026-10-09
- Authorship: recorded by the commit sign-off
- Covers: retained drawing, independent recording, ordered submission and paced delivery

## Sources and scope

The guest contracts remain those recorded in signatures 0002, 0003, 0009 and
0010. The implementation combines the original experiments in provenance
0034 and 0036. No new guest layout, enumeration or synchronization meaning is
inferred. All measurement scenes, shaders and test data are original.

The host synchronization rules follow the public Vulkan
[command buffer specification](https://docs.vulkan.org/spec/latest/chapters/cmdbuffers.html),
[synchronization specification](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
and the presentation sources listed in 0034-presentation-timing.
[Queue submission batches](https://docs.vulkan.org/refpages/latest/refpages/source/vkQueueSubmit.html)
define batch order and the scope of each signal operation.
[Host timeline waits](https://docs.vulkan.org/refpages/latest/refpages/source/vkWaitSemaphores.html)
define completion queries and bounded waits. No external implementation code
was copied for this integration.

## Ownership and reuse

Recording ownership remains private to each command buffer. Completed lists
publish independently and transfer once to the execution owner. Consumed
vector capacity returns through a separate atomic slot. Recycling an older list
does not acquire or overwrite a newer active recording. The next recording
adopts the returned capacity when it is larger than its current storage.
The hot append path borrows its cached recorder reference instead of changing
its shared reference count for every command. The atomic lease still rejects
simultaneous use of that buffer.

Submissions with multiple guest recordings use private worker pools for clears.
A single-recording clear retains its native command slot, avoiding a worker
round trip. General image copies and native presentation use private worker pools.
Callback readbacks, source transitions, grouped draws,
sampled-image preparation and arena transfers retain reusable native command
buffers. Cached transfers retain executable native commands rather than copying
captured operations. Direct recording bypasses argument copying and operation
allocation. Geometry append and native draw recording remain free of allocations
per draw. Descriptor records remain immutable and complete resource identities
still select them. Both push descriptors and ordinary descriptor sets remain
supported.

Adjacent retained submissions collect in a bounded batch of at most sixteen.
Each keeps its completion threshold and dependencies. A worker job, checkpoint or
presentation action flushes preceding retained work before taking its ticket.
A completion wait or incomplete poll also flushes pending retained work. An already completed slot
can be reused without flushing unrelated pending work. The submission owner
combines timeline-only commands into one submit record. Each distinct timeline
signals its highest value at the end of the batch, satisfying all earlier
thresholds. Binary dependencies keep separate submit records in their original
order. Sixteen image
command slots retain their framebuffer and distinct immutable draw states until
retirement. Sampling transitions and grouped draws no longer force an immediate
host wait. Slot reuse still waits before resetting a native command buffer.
A single shared failure record replaces completion allocations for each
retained command; GPU timelines remain the completion authority. Callback
readbacks additionally acknowledge driver submission before a blocking GPU
wait, and flush when enqueued so queued delivery does not delay GPU issuance.

A single submission owner accepts worker results, retained native buffers,
timeline checkpoints and native presentation requests in one ticket sequence.
A direct command cannot pass an earlier worker that is still recording.
Worker assignment counts recording jobs separately, so direct commands and
checkpoints do not consume worker rotation positions. Each command slot signals
its increasing timeline value, with binary semaphores retained for native
acquisition and presentation. Native presentation keeps its retirement fences.
CPU arena access first waits for the last issued command's timeline, which
also proves retirement of the preceding ordered prefix. A shared condition
variable acknowledges issuance before this host wait. The semaphore stays owned
through the wait. Work without a command timeline uses an empty checkpoint.
A previously retired prefix avoids both waits. A new queued command invalidates
that proof. Successful retirement requires no queue or device idle
call. Failure drains pending GPU work before publishing failed completion or
submission acknowledgments.

Packed, unscaled two-dimensional RGBA callbacks copy canonical arena bytes
on the GPU into the private readback buffer. If the image needs refreshing, its
load still follows this copy before later sampling. Other same-size callbacks
copy the source image directly into the private buffer, with the readback owning
the source transition. Scaling retains the intermediate image and blit path.
The rules follow the public Vulkan
[buffer copy specification](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdCopyBuffer.html)
and [image-to-buffer copy specification](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdCopyImageToBuffer.html).

A completed image load or store records the arena write revision, full binding
identity and packing. Callback preparation can reuse that image only while all
three remain unchanged. CPU uploads and writes, allocation changes, compute dispatches,
draws and other image stores advance the arena revision. Direct native image
uploads and explicit transfer loads invalidate their coherence proof. Revision
overflow disables reuse. The proof avoids immediately reloading pixels already
synchronized with canonical arena storage. It does not infer anything from guest
format integers.
The GPU checks cover CPU mutation and image writes through aliased storage.

The submission owner keeps out-of-order results in reusable ticket slots instead
of allocating an ordering-map node for each request. Retained command slots wait
on their timeline; callback slots additionally wait for its shared issuance
acknowledgment. Bounded waits check submission failures before retrying, so a
failed recording cannot leave a host wait without a completion value.

Worker retirement checks the oldest fence directly instead of first waiting
100 microseconds for another channel message. Guest completion records the
retired submission prefix. A subsequent drain of an unchanged successful prefix
returns immediately. A new submission still requires execution and GPU
completion, and a pending validation error is reported once before a successful
prefix can be reused. The changes retain asynchronous guest submission.

Mapped arena and staging allocations prefer host-cached coherent memory.
The arena breaks ties in favor of device-local memory, and both retain compatible
uncached fallbacks. Profiling the integration found mapped-memory copies were
the dominant CPU cost in the textured presentation loop. The public Vulkan
[memory property specification](https://docs.vulkan.org/refpages/latest/refpages/source/VkMemoryPropertyFlagBits.html)
describes host caching for read and read-modify-write access. The arena's host
upload and download boundaries remain in place. This changes memory selection,
not ownership or visibility requirements.

## Measurement method

The historical numbers below come from 0034-presentation-timing,
0034-concurrent-recording and 0036-draw-costs. Fresh paired measurements use
preserved public revisions `1d04491`, `cbc663d` and `543945a` respectively.
Revision `b894356` supplies an additional serial comparison using the same
recording fixture. Sources and executables are isolated from the working tree.
Build, temporary and driver-cache output use environment-selected storage.

The draw fixture still uses 10,000 draws, 100 state groups, one full untimed
warmup frame and three measured frames. Fresh submit measurements use process
CPU time through explicit completion, including execution, native recording,
submission and driver threads. Historical submit measurements used the calling
thread CPU clock. These clocks must not be treated as equivalent. Paired draw
runs use the same process clock on both implementations. Recording time still
uses the recording thread clock. Pixel assertions and compilation remain
outside the timed intervals.

The latency fixture records a fresh one-shot list for every frame and completes
its draw through a metadata query before starting the present timestamp.
The query drains execution while leaving queued callback delivery unchanged. Its former loop resubmitted a
consumed handle, and its warmup read pixels before asynchronous completion.
The same corrected fixture runs against both preserved and integrated code.
It keeps 32 warmup presents, 512 measured presents, the two-frame queue and an
eight-present mailbox tick. Serial delivery drains after each present. Callback
entry precedes pixel inspection. Latency ends at callback entry and does not
measure scanout.

The recording fixture retains 8,192 viewport commands, one clear per recording
thread, 20 warmup frames and 240 measured frames. It reports process CPU time
and elapsed frame time at 1, 2, 4 and 8 persistent recording threads. Each
submitted frame is explicitly completed. Different thread counts also have
different clear counts, so comparisons use matching thread counts.

Twelve timing samples are paired in alternating order for each primary draw
variant, recording comparison and delivery mode. Extended draw variants have
six samples. Recording order rotates equally through all three implementations. Checks and builds finish before
measurement. The host is not isolated from unrelated activity. Raw measurements
retain every final sample, including slow ones. Medians and ranges describe the
observed noise rather than a guaranteed performance bound.

## Results

The [raw samples](0037-integrated-samples.csv) contain all 288 final rows.
Historical columns reproduce the published experiment. Fresh columns are
medians of the matching final runs. These are synthetic workloads on a shared
host, not performance guarantees for other workloads.

### Draw costs

Historical submit CPU uses the calling thread clock. Both fresh submit columns
use the process clock and include explicit completion. Times are nanoseconds
per draw; throughput is frames per second.

| Variant | Historical submit CPU | Fresh reference CPU | Integrated CPU | Historical FPS | Fresh reference FPS | Integrated FPS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 10,000 array draws, push | 1,052.44 | 950.52 | 443.55 | 54.318 | 58.185 | 101.594 |
| 10,000 array draws, ordinary | 1,228.10 | 1,086.12 | 465.77 | 52.553 | 53.213 | 100.184 |
| 20,000 array draws, push | 626.84 | 532.64 | 299.20 | 42.707 | 50.617 | 79.496 |
| 10,000 indexed draws, push | 1,382.74 | 1,476.70 | 671.52 | 38.240 | 37.901 | 70.530 |

Recording CPU still uses the calling thread clock, in nanoseconds per command.

| Variant | Historical | Fresh reference | Integrated |
| --- | ---: | ---: | ---: |
| 10,000 array draws, push | 95.68 | 76.74 | 73.77 |
| 10,000 array draws, ordinary | 94.90 | 93.08 | 76.97 |
| 20,000 array draws, push | Not recorded | 73.99 | 71.84 |
| 10,000 indexed draws, push | Not recorded | 81.40 | 78.27 |

Warm descriptor updates and pool creations remain zero in every final sample.
Every frame keeps 100 pipeline lookups, descriptor preparations and state binds.
Push variants keep 100 push writes; ordinary sets keep zero. Retained draws
remain 9,900, or 19,900 when the array draw count doubles.

Whole-frame Rust allocation counts include the new publication and retirement
records. Recording uses 13 allocations against the historical and fresh 9.
Submission uses 2,651 against 2,613 for array draws and ordinary sets, and 2,721
against 2,613 for indexed draws. Doubling the array draw count keeps both counts
unchanged. The added costs are outside the reused per-draw path.

### Callback latency

Means are microseconds; variances are square microseconds. Each entry is the
median across runs. Historical entries use the three published runs. Fresh
entries use the corrected scene described above.

| Policy | Historical mean | Fresh reference mean | Integrated mean | Historical variance | Fresh reference variance | Integrated variance |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Serial drain | 142.425 | 150.784 | 58.246 | 1,593.468 | 19,041.694 | 207.219 |
| FIFO, two queued | 3,784.704 | 2,484.677 | 424.264 | 940,751.443 | 714,091.679 | 2,052.837 |
| Mailbox, tick every eight | 154.430 | 150.358 | 58.058 | 1,834.925 | 6,300.745 | 195.234 |

Every serial and FIFO run delivers all 544 submitted frames, including warmup.
Every mailbox run delivers 68 and drops 476. No frame fails; no queue exceeds
two pending frames. Timing uses 512 serial or FIFO callbacks and 64 mailbox
callbacks after warmup.

### Independent recording

CPU time includes all process threads. Frame time includes completion.
Historical entries are medians of the published concurrent implementation.
Fresh reference entries use revision `543945a`.

| Recording threads | Measurement | Historical | Fresh reference | Integrated |
| ---: | --- | ---: | ---: | ---: |
| 1 | CPU ms/frame | 0.749523 | 0.752497 | 0.560065 |
| 1 | Frame time, ms | 0.815260 | 0.814084 | 0.620414 |
| 1 | FPS | 1226.602 | 1228.682 | 1612.023 |
| 2 | CPU ms/frame | 1.180257 | 1.228281 | 1.001521 |
| 2 | Frame time, ms | 0.727938 | 0.782781 | 0.631957 |
| 2 | FPS | 1373.744 | 1277.513 | 1592.582 |
| 4 | CPU ms/frame | 1.910191 | 2.178086 | 1.640875 |
| 4 | Frame time, ms | 0.841238 | 0.945392 | 0.624671 |
| 4 | FPS | 1188.725 | 1057.779 | 1600.872 |
| 8 | CPU ms/frame | 2.739963 | 2.741831 | 2.316352 |
| 8 | Frame time, ms | 1.187462 | 1.215601 | 0.552499 |
| 8 | FPS | 842.132 | 822.720 | 1810.123 |

The additional one-thread comparison uses the earlier serial implementation,
revision `b894356`, with the same final recording fixture. It removes the
previously reported 11% CPU increase and 8% throughput loss in this scene.

| One-thread measurement | Historical serial | Fresh serial | Integrated |
| --- | ---: | ---: | ---: |
| CPU ms/frame | 0.677086 | 0.691837 | 0.560065 |
| Frame time, ms | 0.752526 | 0.800044 | 0.620414 |
| FPS | 1328.858 | 1250.358 | 1612.023 |

### Paired noise checks

Changes below are medians of integrated/reference ratios for matching sample
numbers. The 95% intervals use 10,000 bootstrap resamples of whole pairs,
percentile endpoints and seed 4. They measure variation between runs.
An interval crossing zero does not resolve a change at this sample size.
All requested timing comparisons improve outside these intervals. Secondary
command-recording timings improve or remain within observed noise.

| Measurement | Paired median change | 95% interval |
| --- | ---: | --- |
| 10,000 array draws, push, submit CPU | -53.4% | -57.2% to -47.3% |
| 10,000 array draws, push, record CPU | -3.7% | -11.5% to -1.5% |
| 10,000 array draws, ordinary, submit CPU | -56.7% | -60.0% to -48.4% |
| 10,000 array draws, ordinary, record CPU | -16.4% | -21.0% to +3.6% |
| 20,000 array draws, push, submit CPU | -46.5% | -51.8% to -39.5% |
| 20,000 array draws, push, record CPU | -3.9% | -20.6% to +5.1% |
| 10,000 indexed draws, push, submit CPU | -52.7% | -64.8% to -46.3% |
| 10,000 indexed draws, push, record CPU | -3.7% | -29.5% to +24.3% |
| 1 recording thread, CPU/frame | -24.6% | -29.1% to -19.5% |
| 1 recording thread, frame time | -24.6% | -28.8% to -18.0% |
| 1 recording thread, FPS | +32.7% | +22.5% to +40.5% |
| 2 recording threads, CPU/frame | -19.9% | -35.6% to -12.5% |
| 2 recording threads, frame time | -14.6% | -28.1% to -6.4% |
| 2 recording threads, FPS | +17.1% | +6.9% to +39.9% |
| 4 recording threads, CPU/frame | -24.0% | -32.8% to -22.1% |
| 4 recording threads, frame time | -32.3% | -40.8% to -27.8% |
| 4 recording threads, FPS | +47.8% | +38.6% to +69.2% |
| 8 recording threads, CPU/frame | -15.0% | -20.4% to -13.2% |
| 8 recording threads, frame time | -54.6% | -59.1% to -45.3% |
| 8 recording threads, FPS | +120.5% | +82.8% to +144.3% |
| Serial callback latency | -62.0% | -65.1% to -56.3% |
| FIFO callback latency | -81.7% | -84.4% to -78.3% |
| Mailbox callback latency | -62.4% | -68.1% to -58.9% |
| One thread against serial, CPU/frame | -17.8% | -27.3% to -12.0% |
| One thread against serial, frame time | -17.9% | -29.3% to -7.3% |
| One thread against serial, FPS | +21.8% | +8.5% to +41.4% |

The intervals do not establish the absence of occasional slow frames. The raw
table retains all run summaries. Every callback contributes to its run's mean
and variance, including slow ones. Serial callback variance has a wide paired interval that crosses zero, while FIFO and
mailbox variance reductions resolve outside noise. The central callback latency
improves in all three policies.

## Verification

Formatting and default and all-feature Clippy pass with warnings denied.
Default workspace tests and all-feature documentation tests pass. Both complete
all-target, all-feature runs include ignored GPU and external translator checks.
Each descriptor mode passes 165 tests across the workspace and nested translator
packages, with zero failed or ignored tests. No translator check is skipped.
The C examples compile with C11 syntax checks and warnings denied.

The original pixel, alias, format, depth, raster, uniform, indexed drawing and
presentation tests pass. Draw reuse tests preserve warm descriptor counts and
validate partial completion before an invalid command. New tests prove that
recycling cannot overwrite an active recording, direct commands cannot overtake
a blocked worker, worker recording continues independently, and a successful
retired prefix avoids empty handoffs while reporting a validation error once.
The corrected latency example checks exact delivery counts for each policy,
queue bounds and pixels on every delivered frame.
