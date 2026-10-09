# 0032: startup integration and first-draw measurements

- Date: 2026-10-09
- Author: Khargoosh
- Covers: startup cache integration, synthetic move fixtures, test storage and GPU timing

## Evidence and integration

Guest layouts, signatures and values retain the evidence in notes 0021, 0023,
0027 through 0031 and the repository's signature, shape and census records.
This integration adds no guest enum mappings or shader instruction meanings.
Startup cache loading, owned translation workers and saved pipeline recipes
now coexist with the tiled texture transfer implementation.

The immediate move fixtures omitted the four lane-mask bits at positions 12
through 15. Mesa sets this field when encoding an immediate move in
`src/nouveau/compiler/nak/sm50.rs:1938-1941`. The instruction table in
`envydis/gm107.c:2170` independently identifies that four-bit field.
The fixtures now set all four bits. Their other instruction encodings retain
the provenance recorded in notes 0023, 0025 and 0027.

Before the correction, translated fragment programs stored zero color and
translated memory programs left sentinel bytes untouched. After the correction,
the existing exact pixel, memory-copy and compute-pipeline tests pass with the
current translator. No translator implementation changed.

The original CPU cache compatibility test and the GPU startup test share one
adapter of the translator's public cache and translation interfaces. The adapter
preserves subgroup requirements and supplies default patch and texture state
for these original programs. Compilation checks the public interface directly.
The CPU proof now runs against the configured translator without a compatibility
skip or generated substitute implementation.

## Storage and publication

Generated translator test packages use `CARGO_TARGET_DIR`. Shader outputs and
test cache directories use the operating system temporary directory, which
honors `TMPDIR`. The drawing check preserves supplied build, driver-cache and
temporary directories. Generated manifests and dependency paths remain runtime
artifacts. The test wrapper propagates release mode to its generated package.

Account references and machine paths were removed from metadata and CI.
Public header evidence uses the header filename. Symbol lookup and shape parsing
use the observed function table or a generic identifier parser. The observed
function symbols remain unchanged.

## Timing method

Each of 40 trials prepares a warm application cache in an untimed fresh process.
It then measures a fresh cold process with an empty application cache and a
fresh warm process with the prepared cache. Trials alternate cold-first and
warm-first order. Each trial has separate cache directories, which are removed
before the next trial. The implicit Mesa shader disk cache is disabled in every
child. The application startup index supplies warm translation hits.

Each child builds original passthrough vertex and constant-color fragment
programs. The timer starts before translator adapter creation and
`Instance::with_host_startup_cache`. It includes Vulkan creation, translation
index and driver-cache loading, startup pipeline scheduling, host setup, shader
registration, draw submission and completed host readback. Pending draws retry
at one-millisecond intervals. The timer stops after the first completed colored
draw. Exact triangle pixels are then checked. Process launch and shutdown,
fixture byte generation and the final exhaustive pixel check are excluded.

Every cold child asserts zero loaded shaders, two cache misses, two translations,
no startup pipeline and no loaded driver cache. Every warm child asserts two
loaded shaders, two cache hits, zero translator calls, one pipeline queued during
construction and a loaded driver cache. Both assert exactly one pipeline miss,
so the warm draw reuses its queued pipeline instead of compiling a second one.
Both require empty diagnostics and correct output.

These checks establish cache reuse and pipeline scheduling. They do not prove
that the driver avoided all compilation. Public Vulkan documentation describes
[pipeline cache retrieval](https://docs.vulkan.org/refpages/latest/refpages/source/vkGetPipelineCacheData.html)
and [graphics pipeline creation](https://docs.vulkan.org/refpages/latest/refpages/source/vkCreateGraphicsPipelines.html).

## Release results

The parent and generated fixture both used release mode. The run used one
discrete Vulkan GPU and Mesa 26.2.4. The raw samples are in
[0032-startup-samples.csv](0032-startup-samples.csv).

| Startup through completed first draw | Cold | Warm |
| --- | ---: | ---: |
| Samples | 40 | 40 |
| Median | 40.418 ms | 33.536 ms |
| First quartile | 33.168 ms | 32.908 ms |
| Third quartile | 41.603 ms | 35.449 ms |
| Interquartile width | 8.435 ms | 2.541 ms |
| Minimum | 31.591 ms | 26.774 ms |
| Maximum | 54.970 ms | 49.366 ms |

Quartiles are medians of the lower and upper halves. Warm startup reduced the
median by 6.882 ms, or 17.0%, and was faster in 27 of 40 paired trials.
The median paired reduction was 6.321 ms. Resampling whole trial pairs 20,000
times with seed 62026 gives a percentile 95% interval of 4.218 to 7.446 ms for
the difference between group medians, and 0.846 to 7.426 ms for the median paired
reduction. Both intervals are above zero. These results support faster warm
startup for this synthetic workload on this host.

The cold median summed worker translation time was 0.485 ms; warm workers did
no translation. Pipeline scheduling and cache reuse also contribute to startup,
so the total reduction must not be attributed entirely to shader translation.
Filesystem page caches and hardware caches were not flushed. The full ranges
overlap, and the result does not establish a speedup for every draw or workload.

## Verification

The default workspace suite and the all-feature workspace suite, including
ignored GPU and translator tests, passed with exit code 0. Coverage includes
startup cache import and runtime append compatibility, restart reuse, exact
graphics pixels, uniforms, texture transfers and translated memory and compute
programs. The compressed-format test explicitly reported optional unsupported
formats; it completed its supported-format checks. No executable test was left
ignored in the all-feature run.

Workspace formatting, explicit fixture formatting and warnings-denied clippy
passed for both workspace feature configurations and the generated drawing and
CPU compatibility packages. The release first-draw experiment passed all 80
measured children and 40 cache-seeding children. The stored observation parser
still reports the three evidence files and their 57 selected fields. The
drawing verification script also passed its four texture and blend checks with
the configured build and temporary directories.
