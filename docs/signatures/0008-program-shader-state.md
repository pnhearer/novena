# Signatures 0008: retained shader state

Source: [signatures 0007](0007-program-shaders.md) and its cited shape
records. This entry describes only what novena retains; it does not assign
meanings that the observations do not establish.

`ProgramSetShaders` supplies a pointer to one or more records. For each
record, novena retains the record address, the eight observed 64-bit words,
and the two wide values at `+0x00` and `+0x30` as GPU-shaped addresses. The
record is kept in novena's side table keyed by the program object address.

The observations do not establish which GPU-shaped value points to shader
code, the code size, the stage represented by a record, or whether the
record stride is `0x40` when the count is not one. Novena therefore does not
read shader code or call a translator yet. Further observation needed:
varying counts, labelled records for each count, and shape-only observations
of the memory at both GPU-shaped values sufficient to identify code and its
length.
