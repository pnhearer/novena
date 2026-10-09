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

## Results and verification

Formatting and strict default and all-feature Clippy pass. The input-subset
identity regression check and the asynchronous submission-completion check pass.
The release benchmark examples build.

Complete workspace validation and fresh measurements remain pending. The test
process could not create its temporary files in the supplied scratch storage.
Those failed runs establish no timing result and do not establish a passing
GPU or translator suite. One independent fixture failure was corrected by
checking execution status at completion instead of submission acceptance.
No fresh result is substituted for the historical measurements linked above.

