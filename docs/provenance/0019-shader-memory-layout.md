# 0019: shader memory layout

- Date: 2026-10-07
- Author: Khargoosh
- Covers: shader memory reads, shader translation input, and the shader-memory observation

## What was learned

The maintainer's shape-level observation covered 1,564 shader records. The
`+0x00` GPU-shaped value resolved through registered pools for 1,554 records;
`+0x30` never resolved. At the location from `+0x00`, word 0 was nonzero,
words 1 through 11 were zero, and header-like small values began at `+0x30`.

Skipping `0x30` bytes and then an 80-byte shader program header reached real
instruction words in every case. Skipping zero bytes produced an all-zero
first instruction in 833 cases. The resulting shape is a `0x30`-byte prefix,
an 80-byte header, and code beginning at `+0x80`.

The prior zero-run hint cut 686 buffers short inside the prefix or header.

## How

This is a maintainer-provided shape-level observation from real runs. No
program content, instruction words, header values, or addresses are recorded.
The new fake-host tests reproduce only the observed byte layout with synthetic
values.

Novena now searches for sixteen consecutive zero words starting at `+0x80`,
bounded by 64 KiB and the end of the resolved pool. It passes the translator
the bytes beginning at `+0x30`. This offset choice is novena's implementation
choice based on the observation, not a newly observed API contract.

## Confidence and open questions

The offsets, counts, and pool-resolution results are observations. The choice
of the first GPU-shaped value, the translation boundary at `+0x30`, and the
zero-run rule are implementation choices supported by those observations.
The meanings of the prefix and header remain open.
