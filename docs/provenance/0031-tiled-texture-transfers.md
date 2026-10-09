# 0031: tiled texture transfers

- Date: 2026-10-08
- Author: Khargoosh
- Covers: image packing, CPU conversion, compute transfers, compressed images,
  mip sampling, recorded image copies and blits

## Evidence and source boundary

[Signatures 0003](../signatures/0003-objects.md#textures) establish builder fields
for flags, target, format, dimensions, level count, stride and storage offset.
The numeric meanings of flags, target and format are unresolved. Recorded
stride values do not establish a tiling selector.
[Signatures 0006](../signatures/0006-copies.md) leave copy pointer layouts,
rectangles and argument order unresolved.
[Signatures 0001](../signatures/0001-setup.md) record LOD setters without varying
values, so their float order remains a host hypothesis.

`ImageContract` therefore matches complete flags/target/format tuples supplied
by the host. A rule explicitly chooses image kind, host format and linear or
tiled storage with maximum height and depth exponents. Unknown tuples fail.
For array and cube rules, the depth field means layer count; for a 3D rule it
means image depth. These interpretations are explicit choices, not new observed
guest facts. Nonzero stride accepts only a tight, single-level linear image.
No private descriptor bits or opaque pointer structures are decoded.

A host `CopyDecoder` receives all eight recorded integer registers and returns
a typed copy or blit. The library retains raw values without following pointers.
The synthetic command test supplies its own decoder and sentinel references.
It proves recording and execution, not the meaning of an observed pointer.
Without a decoder, the existing whole-image source/destination assumption remains.

The MIT-licensed [open-source GPU driver library image implementation](https://github.com/chaotic-cx/mesa-mirror/blob/mesa-24.0.0/src/nouveau/nil/nil_image.c)
and its [tiling definitions](https://github.com/chaotic-cx/mesa-mirror/blob/mesa-24.0.0/src/nouveau/nil/nil_image.h)
provide public block geometry, level minification, ceiling tile sizes and array
stride alignment. The [public tiled-surface documentation](https://envytools.readthedocs.io/en/latest/hw/memory/g80-surface.html)
provides block ordering, effective mip tile clamping and sequential array and
cube subtextures. These sources inform packing; they do not establish guest
builder enum values. The fixed sector permutation below is the explicit storage
layout implemented here. Other within-GOB permutations are unsupported.

The [Vulkan format specification](https://docs.vulkan.org/spec/latest/chapters/formats.html)
provides compressed block extents and sizes.
[Copies and blits](https://docs.vulkan.org/spec/latest/chapters/copies.html)
provide transfer alignment, compressed region edges, filter requirements and
numeric-class restrictions. Only the permitted recorded evidence and public
sources informed the guest interpretations. Conversion code and test patterns
are original implementations.

## Packing and conversion

One GOB contains 512 bytes, with 64 byte columns and eight rows. For byte column
`x` and row `y` inside a GOB, the byte offset is:

```text
(x / 32) * 256 + (y / 2) * 64 + ((x % 32) / 16) * 32
    + (y % 2) * 16 + x % 16
```

Blocks contain one GOB in width, `2^h` GOBs in height and `2^d` in depth, with
exponents zero through five. Blocks advance in x, then y, then z. GOBs advance
in y, then z within a block. A mip dimension halves with floor rounding and a
minimum of one. Compressed dimensions round up to whole format blocks before
packing. Effective tile exponents shrink only while half the current tile
still covers the level. Each level starts at its effective tile alignment;
the complete mip chain rounds up to the effective base-level tile for each
array layer. Cube faces use the array layout, require square extents and have
a layer count divisible by six.

`Layout` owns checked offsets, strides and sizes for both representations.
Linear scratch packs layers and mips with 16-byte alignment. Padding is not
image content. Forward and inverse conversion touch only active bytes, leaving
caller padding and range guards intact. A golden case independently checks
17 rows, effective height exponent two and the following mip's byte offset.

The CPU kernel copies complete 16-byte sectors using a precomputed GOB table.
The full-sector permutation is byte addressed for element sizes 1, 2, 4, 8 and
16. Row widths and edge counts are computed once. Separate element-size kernel
copies would duplicate this loop, so there is one sector implementation.
Large, suitably aligned output uses SSE2 streaming stores and a completion
fence. Encode traverses eight source rows across block columns. Decode reads
consecutive tiled GOBs. For a 16-byte-offset output, specialized prefix counts
combine sectors from neighboring GOB columns into complete aligned output
cache lines. Cached prefix and tail sectors stay within active rows. Partial
sectors use cached copies. Parallel conversion
splits disjoint 2D block rows, 3D depth slabs or layer/mip ranges. Small images
avoid thread startup. The benchmark explicitly allocates both 64-byte-aligned
and 16-byte-offset buffers; ordinary allocations do not promise cache-line
alignment.

## GPU resources and operations

The compute shader owns one complete destination word per invocation. Narrow
rows gather bytes without overlapping word writers. Masked tiled edge writes
preserve padding; linear scratch padding may be overwritten. It accepts precomputed level plans, including compressed block
payloads. Transfer barriers order compute, image copies and arena access.

The arena stays mapped and prefers coherent, device-local visible memory,
falling back to another compatible visible type if allocation fails. Device
addresses let conversion read or write arena storage directly. Scratch storage,
compute pipelines and upload/readback staging buffers survive transfers. Image
allocations are reused from a pool capped at eight entries. Two command slots
retain executable recordings keyed by every image, buffer, address, packing
plan, direction and starting image layout. Each slot waits for its own fence
before reuse. Image removal and scratch growth invalidate cached recordings
before resources can be destroyed or replaced. Batches accept up to 256 images and share one submission and
scratch allocation, with dependencies between transfers.

Images support 2D, arrays, 3D, cube and cube arrays with complete mip chains.
BC1 through BC7 and the LDR ASTC formats retain their encoded payloads. Native
image creation checks device format support; unsupported formats fail rather
than being reinterpreted. Sampled float descriptors retain their reflected
view shape. Mip filters and finite LOD bounds require an explicit sampler
contract. Comparison sampling, multisampling and registered subresource views
remain outside this path.

Whole-image copies cover every mip and layer. Region copies require equal
format and extent, checked mip/layer bounds and valid compressed block edges.
Blits check source and destination support, numeric class and filter support.
Compressed blits and same-object region transfers are rejected. Recorded copy
and blit results are written back to canonical arena storage.

## Exactness checks

The CPU tests compare serial and parallel conversion, edge bytes, padding,
compressed blocks, effective mip tiles and independent offsets.
The required compute test exercises 36 layouts in both directions with an
interior arena offset and range guards.

`texture_images` builds original patterned images at several block heights,
odd sizes and mip counts. It converts arena bytes to images, samples every
texel and mip through the matching 2D, array, 3D, cube or cube-array view, and
compares exact packed pixels. Readback and inverse conversion check every
active byte and untouched padding. Further cases cover compressed payloads,
region copies, nearest blits and recording through command-buffer submission.
A batch replay case changes arena contents between cached loads, checks inverse
stores and verifies both surrounding guards. Twelve image releases exceed the
pool limit; recreating them checks recording invalidation and image reuse. A recorded view-offset query
checks a nonzero array layer and mip.
BC1 through BC7 native cases passed. This device exposes no native ASTC image
support, so ASTC geometry and conversion are covered but native ASTC sampling
is unverified. GPU tests run with the validation layer enabled.

## Throughput

Run the reusable benchmarks from the project root:

```sh
cargo run --release --example texture_cpu --no-default-features
cargo run --release --features vulkan --example texture_gpu
```

Rates count useful image bytes once per direction, in decimal GB/s, excluding
padding. CPU timing includes conversion and parallel thread startup. GPU
end-to-end timing includes conversion, image transfer, submission and completion,
starting from an already populated arena. Compute-only timing uses device
queries and excludes image copies and host submission; it is a lower-bound
comparison workload, not a full resident image-transfer measurement. Setup,
allocation, shader compilation and initial host population are excluded. Each
case checks correctness before timing and performs warmup iterations.

The host and device were shared during measurement. Separate timing phases can
run at different clocks or under different contention, including cases where
the end-to-end rate exceeds the compute-only rate. These numbers describe the
measured configurations and are not an isolated proof of the requested ratio.

| Image | GPU load | GPU store | Compute load | Compute store |
| --- | ---: | ---: | ---: | ---: |
| 64 x 64 | 0.303 | 0.296 | 2.676 | 3.109 |
| 1024 x 1024 | 16.234 | 24.784 | 20.368 | 31.923 |
| 4096 x 4096 | 25.301 | 35.677 | 22.055 | 42.293 |
| 256 x 256 x 64 | 22.298 | 33.302 | 20.603 | 41.819 |
| 1024 mip chain | 21.018 | 28.498 | 26.951 | 39.994 |
| 4096 mip chain | 25.083 | 34.858 | 24.156 | 35.984 |
| 64 images of 64 x 64 | 1.787 | 1.984 | 1.613 | 1.918 |

Unbatched 64 x 64 transfers miss the requested two-times throughput gap because
submission dominates. The batched configuration reduces that gap.

The CPU measurements below use four-byte elements and twelve workers for the
parallel columns. The 3D case has height 256 and depth 64. Each mip chain reaches
one texel. Alignment means a cache-line-aligned base or a 16-byte offset from
that base, for both source and destination.

| Image | Alignment | Encode | Decode | Parallel encode | Parallel decode | Copy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 64 x 64 | offset 16 | 35.589 | 31.416 | 34.599 | 31.159 | 62.163 |
| 64 x 64 | aligned 64 | 37.349 | 31.125 | 36.902 | 30.693 | 63.344 |
| 1024 x 1024 | offset 16 | 9.172 | 11.473 | 17.809 | 17.448 | 12.978 |
| 1024 x 1024 | aligned 64 | 11.586 | 12.078 | 22.344 | 17.301 | 13.455 |
| 4096 x 4096 | offset 16 | 8.785 | 7.526 | 20.440 | 18.596 | 9.702 |
| 4096 x 4096 | aligned 64 | 10.262 | 7.735 | 24.092 | 19.201 | 10.321 |
| 256 x 256 x 64 | offset 16 | 6.794 | 6.774 | 11.025 | 10.147 | 11.546 |
| 256 x 256 x 64 | aligned 64 | 8.414 | 7.430 | 18.629 | 15.990 | 10.335 |
| 1024 mip chain | offset 16 | 9.254 | 10.315 | 13.339 | 13.050 | 10.576 |
| 1024 mip chain | aligned 64 | 11.520 | 11.944 | 15.380 | 13.411 | 12.032 |
| 4096 mip chain | offset 16 | 8.200 | 7.324 | 18.513 | 17.948 | 10.378 |
| 4096 mip chain | aligned 64 | 10.030 | 7.816 | 23.053 | 18.489 | 10.485 |

The strict single-thread 8 GB/s target is not met across all cases. Large decode
and offset 3D conversion remain below it. The shared sector kernel also does
not provide distinct element-size specializations. An attempted AVX2 paired
store path and a column-group traversal regressed measurements and were
removed. Parallel large-image conversion exceeds the target in both
directions. These are measured results, not a claim that every speed target
was achieved.

## Verification

Formatting and strict workspace clippy passed. Default features are empty;
the minimal-feature workspace suite passed. The complete all-feature suite
passed with ignored GPU tests enabled and the translator configured. Focused
GPU image tests passed with validation enabled, and the final CPU kernel
passed the independent tiling tests in release mode. Translator checks use the
externally configured `NOVENA_SHADOWBOX_PATH`.
