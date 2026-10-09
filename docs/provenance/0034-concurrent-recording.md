# 0034: concurrent recording and ordered submission

- Date: 2026-10-09
- Author: project contributor
- Covers: recording ownership, native recording workers, queue submission and completion

## Sources

Guest recording and submission arguments come from
[recording signatures](../signatures/0002-command-buffer.md) and
[object signatures](../signatures/0003-objects.md). No new guest enumeration,
pointer layout or execution meaning is assigned.

The public Vulkan [command buffer specification](https://docs.vulkan.org/spec/latest/chapters/cmdbuffers.html)
requires external synchronization of command pools and completion before
resetting pending command buffers. The public
[synchronization specification](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
defines queue submission order, memory dependencies and fences. The code,
fixtures and measurements here are original project experiments.

## Ownership and order

Each guest buffer owns its recording state. Atomic ownership transfers protect
that state without a mutex, polling or thread affinity. Separate buffers make
progress independently. A simultaneous call on one buffer fails immediately.
Calls outside an active recording retain their accepted no-op behavior.
Completed lists publish separately, so claiming an older list cannot collide
with recording a newer list on the same buffer. They keep the existing handle
encoding, one-shot consumption and 64-recording retention window. Immutable registry snapshots and per-thread
lookup caches keep the shared object table out of the recording append path.
Finalization closes old generations and removes their registry entries.

Guest submission copies the handle array and claims the owned command lists
before returning. It does not take the backend mutex, wait for a fence or call
the driver. One execution thread interprets submissions in arrival order.
Uploads and downloads remain ordered there so a later upload cannot overtake
an earlier GPU write. Recorded state bindings retain their existing snapshots.
The stride settings used by counted bindings are published as CPU metadata,
so recording does not wait for the backend during driver execution.

Two designs were compared. Full immutable draw and copy plans would replace
every interpreter path. Owned native operation streams reuse the existing
interpreter, alias handling and resource lifetime boundaries. The latter was
selected. Guest interpretation remains serial; native command recording runs
in parallel.

Native argument arrays and nested render-pass or barrier data are owned.
Workers receive these streams and record actual Vulkan command buffers. Each
worker owns one command pool, one command buffer and one fence. It resets its
pool only after retirement. The worker count follows available CPU parallelism,
bounded from two through eight workers. Cached transfers reuse their owned
operation lists.

One submit thread owns internal queue submission and native presentation.
It holds worker results by ticket and submits the next ticket only. Every
recorded stream starts with a memory dependency on preceding queue work.
Fences retire in submission order and release workers for pool reuse.
A failed recording publishes its ticket instead of leaving a gap. Driver
failure drains pending work and prevents reuse of failed completion state.

Mapped memory access, draw resource release, scratch growth and resource
destruction drain the captured prefix before driver idle waits. Existing
frame-slot waits now refer to actual replay completion. Instance destruction
joins execution before callbacks or resources become invalid. Memory callbacks
may run on the execution thread. Completion reentry on that same instance
returns an internal error instead of waiting on itself. Submitted memory must
stay valid and unchanged until completion.

Explicit finish, presentation and resource calls observe pending execution
errors. The first pending validation error is reported and then cleared.
Driver errors remain fatal. The CPU fallback keeps its synchronous execution.

## Synthetic measurement

Run from the project root on a Linux host with a Vulkan device:

```sh
cargo run --locked --release --example recording_scale --features vulkan
```

The baseline is `b894356` with the same timing fixture added. Each frame records
8,192 viewport commands across 1, 2, 4 or 8 persistent guest threads. Each
thread also records one target binding and one clear of the same 16 by 16
image. One guest thread submits the finished handles in array order and
finishes the queue. The workload includes real driver submission and completion
on every frame. Setup and 20 warm-up frames are excluded. Each pass times 240
frames; each implementation has three passes.

Process CPU time includes guest recording, interpretation, native recording,
submission and driver threads in the process. Wall time includes synchronization
and completion. Frames per second is measured frames divided by elapsed wall
time. Each table entry is the median of its three measurements. The
[raw samples](0034-recording-samples.csv) retain all timing rows.

| Recording threads | Before CPU ms/frame | After CPU ms/frame | Before FPS | After FPS |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 0.677 | 0.750 | 1328.858 | 1226.602 |
| 2 | 2.838 | 1.180 | 598.834 | 1373.744 |
| 4 | 5.978 | 1.910 | 482.536 | 1188.725 |
| 8 | 13.998 | 2.740 | 375.228 | 842.132 |

At eight recording threads, process CPU time falls about 80% and throughput
rises about 2.24 times. At one thread, CPU time rises about 11% and throughput
falls about 8%. The execution handoff has a measurable serial cost.

Most recorded commands are state updates. Clears force native work, but these
numbers do not predict drawing or transfer throughput. More recording threads
also mean more clears per frame. Comparisons are between the same thread count,
not between workloads with different counts. Other work shares the host.

## Verification

The project tests cover separate-buffer recording on eight threads, ownership
migration, one-shot publication during concurrent recording, immediate rejection
of overlapping use and recovery after a recording panic. A blocked execution owner cannot hold up
guest submit. A deliberately delayed first native stream lets another worker
record, while its completion remains ordered. Repeated submissions reuse the
worker pools. A failed recording unblocks later tickets with an error.

The complete test suite, including ignored GPU tests and translator checks,
passes on the real device. Pixel, arena alias, depth, indexed drawing, shader,
texture transfer and offscreen presentation checks pass. Checks that read
results from guest submission now wait for explicit completion before comparing
bytes. Formatting, strict Clippy with and without the graphics feature, and
the C host syntax check pass. The fallback test lane also passes without an
available Vulkan device. These checks establish this implementation's behavior;
they do not establish additional guest synchronization meanings.
