# 0039: combined execution and timing

- Date: 2026-10-09
- Authorship: recorded by the commit sign-off
- Covers: ordered recording, paced delivery, deferred commands and reusable compilation

## Sources and scope

The integration preserves the contracts and limits recorded in
[recording and presentation](0037-integrated-execution.md),
[deferred commands](0037-command-execution.md),
[pipeline libraries](0037-graphics-libraries.md) and
[frame tails](0038-frame-tails.md).
Guest evidence remains the existing signatures, shapes, census and provenance.
The integration adds no guest layout, enumeration or synchronization meaning.
All shader sources, scenes and data used for verification are original project
experiments. No external implementation code was copied.

## Integration boundaries

Submission retains independent recording ownership and one ordered execution
owner. Deferred compute, indirect draws, counter reports and render predicates
execute on that owner through the existing explicit host contract. Submission
acceptance remains asynchronous; completion reports execution errors. Fixtures
wait for completion before checking status, memory or pixels.

Synchronous buffer operations use real reusable native command buffers. They
submit through the same ordered owner as retained draws. Occlusion collection
retires preceding submissions before reading the query pool, so a retained draw
cannot remain buffered while the host waits for its query result. Arena writes
from buffer fills, copies, compute dispatch and reports invalidate callback
image reuse. Existing callback queues, timeline retirement and retained draw
batching remain in place.

Compilation keeps startup subset creation, retained shader modules, worker
linking, bounded admission and one shared wait deadline. Input-library identity
includes the per-instance fetch mask as well as attributes, strides and
topology. Changing fetch rate cannot reuse an incompatible input library.
The subset-key check first failed when the mask was omitted and passed after
its inclusion. Job equality and hashing use only immutable job identity;
retained device resources remain compilation payload.

Arena download retains ordered timeline completion and host-cached coherent
memory selection. Compilation-stage timing measures that completion boundary,
including its wait, rather than replacing it with device idle. Upload and
download timing spans run on the execution owner.

## Measurement method

Measurements use release binaries on the same physical device as the earlier
notes. Build output, temporary data and driver caches use environment-selected
storage outside the source tree. Compilation and checks finish before timing.

The draw fixture records 10,000 draws in 100 state groups, warms one full frame
and measures three frames. Additional variants force ordinary descriptors,
double array geometry or use indexed geometry. Recording uses thread CPU time;
submission uses process CPU time through explicit completion. Pixel assertions
remain outside timing. Comparisons with 0037-integrated-execution use the same
clocks; the calling-thread submit clock in 0036-draw-costs is different.

The callback fixture uses 32 warmup presents and 512 measured presents with
a two-frame queue. Serial delivery drains each present, FIFO drains the queue,
and mailbox delivery ticks every eight presents. Latency ends at callback
entry. It does not measure display scanout. Each mode checks delivery counts,
queue bounds and pixels.

The recording fixture distributes 8,192 viewport commands across 1, 2, 4 and 8
persistent threads, with one clear per thread. It excludes 20 warmup frames and
measures 240 completed frames. CPU time includes all process threads. Each
thread count has a different clear count, so historical comparisons use the
same thread count.

Pipeline timing launches five fresh processes with 24 translated fragment
variants per process. Each variant uses an empty application cache, disables
driver disk shader caching and verifies its completed color. Setup, translation
and startup subsets precede timing. First use includes retries and their
one-millisecond sleeps. A spike is the longest recording-plus-completion
interval for that variant. Pooled p50, p95 and p99 use nearest ranks over all
120 observations. This matches the method in 0038-frame-tails.

## Results

The measured execution code is revision `644ea31`. Twelve release runs cover
each draw variant, each recording thread count and each callback policy.
The [combined samples](0039-combined-samples.csv) retain 48 draw rows,
48 recording rows and 36 callback rows. Pipeline timing retains all 120
variant observations in [frame samples](0039-frame-samples.csv) and all 85
process-stage rows in [stage samples](0039-stage-samples.csv).

The earlier columns below reproduce the integrated results in provenance 0037
or the final frame-tail results in provenance 0038. They were measured earlier,
not alternated with these runs. The host also had unrelated activity during
this collection. These historical comparisons do not isolate the effect of
integration or establish performance parity. Every final sample is retained,
including slow samples. No new reference implementation was measured.

### Draw cost

Each entry is the median of twelve runs. Submit CPU uses process CPU time
through completion; recording uses recording-thread CPU time. Times are
nanoseconds per draw or recorded command as labeled.

| Variant | Earlier submit CPU ns/draw | Combined submit CPU ns/draw | Combined recording ns/command | Combined FPS |
| --- | ---: | ---: | ---: | ---: |
| 10,000 array draws, push | 443.55 | 894.68 | 110.96 | 56.848 |
| 10,000 array draws, ordinary | 465.77 | 904.28 | 120.48 | 55.853 |
| 20,000 array draws, push | 299.20 | 533.75 | 107.81 | 46.885 |
| 10,000 indexed draws, push | 671.52 | 1,350.32 | 127.47 | 34.998 |

Submit CPU ranges are 801.44 to 1,169.16 for array draws, 813.00 to 1,056.10
for ordinary sets, 491.91 to 566.87 for doubled array geometry, and 1,118.27 to
1,532.18 for indexed geometry. Fresh CPU costs are higher than the earlier
integrated measurements. The retained draw path still performs 100 pipeline
lookups, descriptor preparations and state binds per frame. Warm descriptor
updates and pool creations remain zero. Push variants use 100 push writes;
ordinary sets use zero. Retained draws are 9,900, or 19,900 when geometry doubles.

Recording uses 13 whole-frame Rust allocations in every sample. Submission
uses 2,652 for array draws and ordinary sets, and 2,722 for indexed draws.
Doubling array geometry leaves both allocation counts unchanged. These counts
include frame publication and completion; they are not allocations per draw.

### Present latency

Each entry is the median of twelve process means or variances. Latency ends at
callback entry and includes backpressure. It is not scanout timing.

| Policy | Earlier mean us | Combined mean us | Combined variance us squared |
| --- | ---: | ---: | ---: |
| Serial drain | 58.246 | 117.734 | 26,030.503 |
| FIFO, two queued | 424.264 | 870.286 | 204,746.371 |
| Mailbox, tick every eight | 58.058 | 113.076 | 10,337.119 |

Serial and FIFO deliver all 544 frames, including warmup. Mailbox delivers 68
and drops 476. Timed samples contain 512 serial or FIFO callbacks and 64 mailbox
callbacks. Every run checks pixels, reports no failed delivery and stays within
the two-frame queue bound. Fresh callback means are higher than the earlier
integrated measurements.

### Multi-thread frame time

Each entry is the median of twelve runs at the matching thread count. CPU time
includes every process thread. Elapsed frame time includes submission completion.

| Threads | Earlier CPU ms/frame | Combined CPU ms/frame | Earlier elapsed ms/frame | Combined elapsed ms/frame |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 0.560065 | 0.880895 | 0.620414 | 1.093609 |
| 2 | 1.001521 | 1.230343 | 0.631957 | 1.087010 |
| 4 | 1.640875 | 1.391715 | 0.624671 | 1.208710 |
| 8 | 2.316352 | 1.646866 | 0.552499 | 1.547700 |

Elapsed frame time is higher in these runs, while process CPU cost at four and
eight threads is lower. Additional threads do not guarantee shorter frames for
this small clear workload. Each thread count retains its original clear count.

### Pipeline first use and p99 spike

Quantiles pool five fresh processes with 24 variants each, using nearest ranks.
Values are milliseconds. Each variant verifies its completed pixel color.

| Interval | Combined p50 | Combined p95 | Combined p99 | Combined maximum |
| --- | ---: | ---: | ---: | ---: |
| First request through completed pixels | 3.507 | 5.731 | 8.742 | 9.972 |
| Longest recording-plus-completion interval | 1.138 | 2.056 | 3.224 | 3.429 |

The earlier final frame-tail experiment measured 2.832 ms first-use p50 and
6.389 ms first-use p99, with 0.398 ms frame-spike p50 and 1.960 ms frame-spike
p99. The combined result has larger values here. It retains the smaller tails
relative to that experiment's earlier baseline, whose frame-spike p50 was
11.015 ms and p99 was 21.374 ms. These remain historical comparisons.

Every fresh pipeline process reports library and fast-link capability, six
startup input parts, one raster part, 24 fragment parts, eight startup output
parts and 24 worker links. Timed variants create no input or output parts.
Module creation remains exactly one vertex module and 24 fragment modules.
Descriptor preparation takes no cache lock. The largest host download call is
3,187.314 us. The largest measured device-completion boundary is 3,117.879 us.
The largest mapped read chunk is 239.587 us. Stage totals overlap and must not be
summed into frame cost. The results do not establish that all frame spikes or
all worker compilation tails are gone.

## Reproduction

Configure the external translator and the build, temporary and driver-cache
storage through `NOVENA_SHADOWBOX_PATH`, `CARGO_TARGET_DIR`, `TMPDIR` and
`XDG_CACHE_HOME`. Select storage outside the source tree. Leave the optional
pipeline wait, pressure and library-disable controls unset for these samples.
Run from the repository root:

```sh
python3 scripts/measure-combined.py docs/provenance
```

The runner builds release examples, runs the fresh-process pipeline fixture,
rotates draw and callback policy order, and writes the three numeric sample
files. It keeps complete command output in build storage and removes its
scratch image. It fails on unsuccessful commands or explicit translator skips.
Driver diagnostics remain separate from the numeric output being parsed.

## Verification and limits

Formatting, default and all-feature Clippy with warnings denied, default tests,
all-feature documentation tests and all-target all-feature tests pass.
The complete all-feature run includes ignored GPU and translator tests, with
no ignored tests or explicit translator skips. Native presentation exercises
both modes, capacity changes, resize and teardown. GPU command tests verify
compute dispatch, indirect draws, buffer operations, reports, occlusion and
conditional rendering against completed memory or pixels.

All 14 translated drawing tests also pass with pipeline libraries disabled.
The one-worker, one-slot pressure fixture passes with a two-millisecond wait
budget, persistence and worker-based linking. Every process in that fixture
verifies eventual pixels, reusable part counts and retained module counts.
The release measurement runner completes all twelve runs and all five fresh
pipeline processes successfully.

The device format catalogue reports 73 exact sampling/readback cases and 29
unavailable formats, including the compressed format family. Native execution
of those unavailable formats is not claimed; storage geometry and conversion
checks still cover their layouts. No validation-layer result is claimed.
These original synthetic scenes do not establish throughput, startup or tail
parity on other workloads or devices. The 120-observation sample gives limited
p99 precision.
