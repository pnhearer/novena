# 0032: sector copies and staging ring

- Date: 2026-10-09
- Author: Khargoosh
- Covers: CPU kernels, small transfer batches, resident timing and exactness

## Sources and scope

[0031](0031-tiled-texture-transfers.md) supplies the geometry and source boundary.
No new guest flags, formats, descriptors or pointer layouts are inferred.
Performance workloads and pixel patterns are original project experiments.
The [Vulkan synchronization specification](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
supplies dependencies between compute, image copies and host access, and fence
completion before command and staging reuse. No driver code is copied.

## CPU changes

Direction and element sizes 1, 2, 4, 8 and 16 select kernel instantiations before
traversal. Element size fixes sector element counts and byte pitches. Complete
GOB copies use constant sector indices. Buffer ranges and edge GOB dimensions
are checked before copying, with no per-texel address checks in the copy loop.

SSE2 provides sector copies. AVX2 combines sectors into paired loads and stores.
Large encode destinations use aligned streaming stores. For shifted output,
the current GOB joins the previous GOB's final sector with its first sector.
Its successor owns the final sector until the block ends. This closes streaming
cache lines before traversal moves across columns. Only complete neighboring
GOBs share sectors. Worker ranges and edge padding remain disjoint.

AVX-512 supplies cached full-row encode loads and cache-line decode for aligned
output. Shifted decode keeps paired sectors. Large GOB rows prefetch future
source columns. Encode groups eight, four, two or one GOB rows for byte pitches
up to 256, 512, 1024 or larger. Groups clamp to tile height. Narrow groups retain
at most 16 KiB of active source rows before moving across columns.

A cycle profile covering all volume element classes places most sampled time
in GOB traversal, source loads and sector assembly. Full-row loads replace
assembled source vectors. An encode experiment that wrote half of a future
cache line before moving across columns regressed. Moving that write to the
successor GOB removed the open streaming line. Wide shifted decode also
regressed and was removed. The table reports remaining misses.

## GPU changes

Independent images up to 256 KiB receive distinct 256-byte-aligned staging
ranges. Conversions and image copies share dependency boundaries and one
submission. Duplicate images or overlapping arena ranges keep the ordered
path. Batches still accept at most 256 images.

The persistent staging allocation has a fixed range per command slot. Its
fence completes before range and recording reuse. Growth waits for every slot
and invalidates recordings before replacement. Smaller batches retain the
allocation's slot stride. Cache keys include the staging range and conversion
mode. Existing image removal and allocation-pool behavior remain covered.

The resident baseline copies packed linear arena storage directly to or from
the same images. It uses the same slots, cached recordings, waits, shapes and
loop counts. Duplicate image destinations and overlapping store ranges have
explicit dependencies. This replaces the compute-only lower-bound comparison
for the resident time ratio.

## Exactness and verification

All original offset, packing, compressed payload, mip, shape, padding,
serial/parallel and GPU sampling proofs remain. The new CPU oracle covers every
element size at offsets 0, 1, 16, 32 and 48 from a cache-line boundary. Odd
heights, depth edges and mips check padding and range guards. Serial and parallel
results match independent tiled bytes. Cases exceed the worker threshold and
cross real worker boundaries.

The batch proof changes arena content between replays, grows staging with
pending submissions, varies batch spans and wraps both slots before waiting.
It checks tiled and resident round trips, guards, duplicate destinations,
overlapping stores, image release and recreation, and invalid ranges.

After merging the fixture corrections and host documentation from main,
formatting and strict all-target clippy pass with minimal and all features.
The minimal suite passes 63 tests. The release CPU oracle passes 10 tiling tests.
The complete all-feature suite passes 140 tests across 31 suites, including
ignored GPU and external translator checks. Every command exits with code 0.
GPU checks run with the Vulkan validation layer enabled. The earlier seven
translator failures came from zero lane masks in old synthetic fixtures. Main
corrected those masks. Startup cache and first-draw checks remain included.
Generated test files and private caches use configured build or scratch storage.

## Throughput

Run from the project root:

```sh
TEXTURE_BENCH_WORKERS=6 cargo run --release --example texture_cpu --no-default-features
cargo run --release --features vulkan --example texture_gpu
```

CPU durations are medians of three timing samples. The example accepts a case
substring as its first argument. `TEXTURE_BENCH_SAMPLES` and
`TEXTURE_BENCH_LOOPS` override counts. Rates count useful bytes once in decimal
GB/s. Setup, pattern generation and preflight checks are outside timing.
Parallel timing includes thread startup. Both buffers are aligned to 64 bytes
or offset by 16 bytes. GPU wall time includes submission and completion. Device
query rates cover a separate compute-only workload. Host and device contention
can affect phases differently. These runs use a shared host.

| Image | Bytes per element | Alignment | Encode | Decode | Parallel encode | Parallel decode | Copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 64 x 64 | 1 | offset 16 | 38.217 | 41.222 | 38.010 | 39.677 | 107.282 |
| 64 x 64 | 1 | aligned 64 | 42.900 | 45.914 | 40.870 | 44.007 | 94.301 |
| 1024 x 1024 | 1 | offset 16 | 14.056 | 14.129 | 14.206 | 14.338 | 21.405 |
| 1024 x 1024 | 1 | aligned 64 | 15.655 | 15.825 | 14.887 | 15.433 | 20.538 |
| 4096 x 4096 | 1 | offset 16 | 9.754 | 9.281 | 21.066 | 18.242 | 10.237 |
| 4096 x 4096 | 1 | aligned 64 | 11.419 | 9.496 | 15.783 | 18.935 | 10.987 |
| 256 x 256 x 64 | 1 | offset 16 | 11.069 | 9.900 | 14.878 | 17.072 | 12.749 |
| 256 x 256 x 64 | 1 | aligned 64 | 11.966 | 12.264 | 19.034 | 17.929 | 13.009 |
| 1024 mip chain | 1 | offset 16 | 10.744 | 10.705 | 11.424 | 11.369 | 13.568 |
| 1024 mip chain | 1 | aligned 64 | 11.904 | 12.235 | 11.991 | 12.438 | 14.763 |
| 4096 mip chain | 1 | offset 16 | 8.538 | 8.209 | 15.369 | 15.488 | 9.866 |
| 4096 mip chain | 1 | aligned 64 | 10.049 | 9.194 | 12.579 | 14.880 | 9.988 |
| 64 x 64 | 2 | offset 16 | 46.851 | 47.970 | 39.422 | 42.271 | 60.092 |
| 64 x 64 | 2 | aligned 64 | 43.077 | 45.736 | 51.469 | 48.327 | 90.067 |
| 1024 x 1024 | 2 | offset 16 | 11.700 | 11.966 | 11.717 | 11.982 | 13.899 |
| 1024 x 1024 | 2 | aligned 64 | 12.642 | 12.269 | 12.014 | 12.030 | 13.947 |
| 4096 x 4096 | 2 | offset 16 | 9.189 | 8.297 | 20.377 | 16.455 | 10.019 |
| 4096 x 4096 | 2 | aligned 64 | 10.724 | 8.754 | 21.378 | 17.406 | 10.253 |
| 256 x 256 x 64 | 2 | offset 16 | 7.974 | 8.365 | 17.887 | 16.602 | 8.529 |
| 256 x 256 x 64 | 2 | aligned 64 | 8.161 | 10.431 | 17.225 | 15.946 | 9.963 |
| 1024 mip chain | 2 | offset 16 | 11.662 | 11.553 | 11.685 | 11.525 | 12.925 |
| 1024 mip chain | 2 | aligned 64 | 12.287 | 12.105 | 11.657 | 11.665 | 13.618 |
| 4096 mip chain | 2 | offset 16 | 8.835 | 8.633 | 18.927 | 15.769 | 10.119 |
| 4096 mip chain | 2 | aligned 64 | 10.267 | 8.900 | 17.969 | 16.181 | 10.232 |
| 64 x 64 | 4 | offset 16 | 40.984 | 46.177 | 41.053 | 25.208 | 74.815 |
| 64 x 64 | 4 | aligned 64 | 29.776 | 32.266 | 27.134 | 31.628 | 49.316 |
| 1024 x 1024 | 4 | offset 16 | 6.590 | 9.490 | 2.705 | 3.616 | 8.346 |
| 1024 x 1024 | 4 | aligned 64 | 8.697 | 11.466 | 12.695 | 11.620 | 9.005 |
| 4096 x 4096 | 4 | offset 16 | 7.414 | 7.116 | 7.857 | 5.452 | 8.716 |
| 4096 x 4096 | 4 | aligned 64 | 8.807 | 7.229 | 6.685 | 6.306 | 8.596 |
| 256 x 256 x 64 | 4 | offset 16 | 6.657 | 6.561 | 4.195 | 4.452 | 6.605 |
| 256 x 256 x 64 | 4 | aligned 64 | 6.664 | 7.806 | 5.418 | 6.774 | 6.871 |
| 1024 mip chain | 4 | offset 16 | 5.693 | 7.555 | 3.338 | 3.373 | 6.998 |
| 1024 mip chain | 4 | aligned 64 | 6.795 | 6.163 | 2.667 | 2.705 | 8.029 |
| 4096 mip chain | 4 | offset 16 | 6.777 | 6.430 | 6.223 | 5.549 | 8.437 |
| 4096 mip chain | 4 | aligned 64 | 7.949 | 7.573 | 8.250 | 6.700 | 8.126 |
| 64 x 64 | 8 | offset 16 | 21.688 | 18.590 | 18.942 | 16.124 | 40.454 |
| 64 x 64 | 8 | aligned 64 | 26.149 | 31.617 | 25.955 | 27.346 | 38.858 |
| 1024 x 1024 | 8 | offset 16 | 7.010 | 7.192 | 4.728 | 3.960 | 6.875 |
| 1024 x 1024 | 8 | aligned 64 | 8.611 | 8.327 | 13.281 | 12.597 | 7.301 |
| 4096 x 4096 | 8 | offset 16 | 8.518 | 7.926 | 18.674 | 13.027 | 9.062 |
| 4096 x 4096 | 8 | aligned 64 | 9.590 | 8.374 | 11.801 | 13.430 | 9.494 |
| 256 x 256 x 64 | 8 | offset 16 | 8.211 | 7.633 | 16.023 | 14.597 | 9.877 |
| 256 x 256 x 64 | 8 | aligned 64 | 9.189 | 8.765 | 17.282 | 15.691 | 9.277 |
| 1024 mip chain | 8 | offset 16 | 6.831 | 6.885 | 10.623 | 8.985 | 8.889 |
| 1024 mip chain | 8 | aligned 64 | 9.991 | 8.037 | 14.001 | 10.626 | 9.439 |
| 4096 mip chain | 8 | offset 16 | 7.290 | 8.241 | 18.823 | 16.142 | 9.351 |
| 4096 mip chain | 8 | aligned 64 | 9.954 | 8.254 | 18.799 | 16.454 | 8.995 |
| 64 x 64 | 16 | offset 16 | 22.610 | 18.618 | 21.691 | 18.420 | 41.020 |
| 64 x 64 | 16 | aligned 64 | 26.230 | 25.479 | 26.818 | 25.691 | 41.944 |
| 1024 x 1024 | 16 | offset 16 | 8.070 | 7.771 | 14.756 | 11.118 | 8.506 |
| 1024 x 1024 | 16 | aligned 64 | 9.372 | 7.922 | 14.588 | 10.125 | 8.034 |
| 4096 x 4096 | 16 | offset 16 | 8.220 | 7.944 | 18.305 | 16.844 | 9.509 |
| 4096 x 4096 | 16 | aligned 64 | 9.954 | 8.723 | 18.798 | 18.466 | 9.949 |
| 256 x 256 x 64 | 16 | offset 16 | 8.601 | 8.033 | 16.146 | 15.572 | 9.554 |
| 256 x 256 x 64 | 16 | aligned 64 | 9.476 | 8.824 | 18.961 | 17.178 | 9.562 |
| 1024 mip chain | 16 | offset 16 | 8.041 | 7.571 | 13.097 | 10.039 | 9.885 |
| 1024 mip chain | 16 | aligned 64 | 9.256 | 7.450 | 13.697 | 11.170 | 9.452 |
| 4096 mip chain | 16 | offset 16 | 8.003 | 8.221 | 18.343 | 16.556 | 9.439 |
| 4096 mip chain | 16 | aligned 64 | 10.396 | 8.370 | 14.693 | 15.988 | 10.157 |

20 of the 60 image/element-size/alignment combinations have a serial
direction below 8 GB/s in this run. The strict all-case serial target is still
unmet. Shared-load changes also affected the copy reference and parallel startup.
The table records that run and does not guarantee throughput. A fresh run after
merging main has two serial misses across the same 60 combinations. Both are
16-byte-element mip chains with output offset by 16 bytes. Decode measures
7.724 GB/s for the 1024 chain and 7.831 GB/s for the 4096 chain. The kernel is
unchanged between these runs; shared host load affects the measured miss count.

| Image | GPU load | GPU store | Resident load | Resident store | Load time ratio | Store time ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 64 x 64 | 0.248 | 0.216 | 0.327 | 0.313 | 1.319 | 1.451 |
| 1024 x 1024 | 11.482 | 16.310 | 57.707 | 52.737 | 5.026 | 3.233 |
| 4096 x 4096 | 23.547 | 34.825 | 325.406 | 240.164 | 13.820 | 6.896 |
| 256 x 256 x 64 | 18.549 | 33.053 | 212.621 | 159.355 | 11.463 | 4.821 |
| 1024 mip chain | 15.398 | 25.290 | 92.812 | 86.007 | 6.027 | 3.401 |
| 4096 mip chain | 25.976 | 38.440 | 232.647 | 216.183 | 8.956 | 5.624 |
| 64 images of 64 x 64 | 3.588 | 4.453 | 3.508 | 4.442 | 0.978 | 0.997 |

For 64 x 64 images, both single transfers and batches measure less than twice
resident time in each direction. Larger cases still exceed that ratio. Separate
phases can run at different clocks or contention levels, including the batch
ratio just below one. Exactness is checked before every timed configuration.
