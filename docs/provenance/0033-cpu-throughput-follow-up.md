# 0033: CPU throughput follow-up

- Date: 2026-10-09
- Author: Khargoosh
- Covers: repeat measurements and discarded CPU kernel experiments

## Sources and method

These are original project experiments using the workloads and byte patterns in
[0032](0032-sector-copies-and-staging-ring.md). The committed baseline is
`0f4178b`. Each sweep covers the same 60 shape, element-size and alignment
combinations. Rates count useful bytes once in decimal GB/s. A configuration
misses the target if either serial direction measures below 8 GB/s.

Run from the project root:

```sh
TEXTURE_BENCH_WORKERS=6 cargo run --locked --release --example texture_cpu --no-default-features
```

Each rate is the median of three timing samples with the example's default loop
counts. Buffers are aligned to 64 bytes or shifted by 16 bytes. The host has
six physical cores and supports AVX2 and AVX-512. Other work shares the host.
All three candidates were tried separately against the committed baseline.
Each candidate passed all 10 release tiling oracle tests before its sweep.
Every oracle and benchmark command exited with code 0.

## Experiments

The decode traversal candidate consumes a complete tile-height group before
advancing columns, instead of groups capped at eight GOB rows. The wide encode
candidate gives the existing AVX-512 kernel priority over streaming AVX2
encoding. The paired decode candidate gives the existing AVX2 kernel priority
over AVX-512 decoding. No candidate changes image geometry or byte packing.

Ratios below are medians of the 60 per-case rate ratios against the first
baseline sweep. A ratio above one means higher throughput. They include both
buffer alignments and all five element sizes.

| Sweep | Serial misses | Copy rate ratio | Encode rate ratio | Decode rate ratio |
| --- | ---: | ---: | ---: | ---: |
| Committed baseline | 2 | 1.000 | 1.000 | 1.000 |
| Complete tile-height decode groups | 3 | 0.966 | 0.952 | 1.001 |
| Prefer AVX-512 encode | 8 | 0.976 | 0.974 | 0.965 |
| Prefer AVX2 decode | 28 | 0.897 | 0.839 | 0.851 |
| Restored committed baseline | 27 | 0.893 | 0.838 | 0.886 |

The first baseline sweep has two misses, both shifted 16-byte-element mip chains.
Their decode rates are 7.724 GB/s for the 1024 chain and 7.831 GB/s for the 4096
chain. The restored baseline has 27 misses with identical source. Its copy
reference is about 11% slower and its serial encode median is about 16% slower.
That variation prevents attributing the candidate differences to kernels alone.
The earlier 20 misses in 0032 are also a measurement of one host run.

## Result

None of these sweeps establishes a consistent improvement across the target
configurations. All three candidate edits were removed. The sector-copy and
staging-ring implementation remains unchanged, including its passed formatting,
strict Clippy, minimal tests, release oracles, GPU checks and translator checks.
A separate texture-image run passes all five tests with no validation errors or
skips. The restored source also passes formatting and the complete CPU benchmark
preflight checks.

The all-case serial 8 GB/s target remains unmet. Future comparisons need paired
baseline and candidate measurements under a steadier host load before another
kernel selection or traversal change is retained.
