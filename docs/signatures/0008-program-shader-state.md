# Signatures 0008: retained shader state

Source: [signatures 0007](0007-program-shaders.md) and its cited shape
records. This entry describes only what novena retains; it does not assign
meanings that the observations do not establish.

`ProgramSetShaders` supplies a pointer to one or more records. For each
record, novena retains the record address, the eight observed 64-bit words,
and the two wide values at `+0x00` and `+0x30` as GPU-shaped addresses. The
record is kept in novena's side table keyed by the program object address.

The observations establish that the first GPU-shaped value locates shader
memory through registered pools. The second does not resolve. The memory
shape and offsets are recorded in [signatures 0009](0009-shader-memory-layout.md).
The stage represented by a record and whether the record stride is `0x40` when
the count is not one remain open.

The ProgramSetShaders observation performs a shape-only census. It reads at
most the first `0x80` bytes at each GPU-shaped value, labels every 32-bit word
with its offset using the existing pointee classifier, and reports only shape
categories. Its length hint searches from `+0x80`, bounded by 64 KiB and the
resolved pool end. Failed bounded reads are reported as unreadable.
