# Signatures 0009: shader memory layout

Source: maintainer shape-level observation of 1,564 shader records.

The `+0x00` GPU-shaped value resolves through registered pools for 1,554 of
1,564 records. The `+0x30` value never resolves. At the location resolved from
`+0x00`, word 0 is nonzero, words 1 through 11 are zero, and header-like small
values begin at `+0x30`.

Skipping the first `0x30` bytes, then the 80-byte shader program header, makes
the translator reach real instruction words in every observed case. Skipping
zero bytes yields an all-zero first instruction in 833 of 1,564 cases. The
observed layout is therefore a `0x30`-byte prefix, an 80-byte header at
`+0x30`, and code from `+0x80`.

The zero-run length hint must begin at `+0x80`. A run of sixteen zero words
inside the prefix or header is not a shader length. The observed zero-run hint
cut 686 buffers short when it searched the whole buffer.

The observation establishes these shapes and counts. It does not establish
the meanings of the prefix or header fields.
