# Signatures 0006: texture copies

The observations establish the register shape only partially. `CopyBufferToTexture`
has a wide value in x1 and readable values in x2 through x4. The later values
are pointers, but no field at their pointees has been identified. The seven
`CopyTextureToTexture` calls had constant values in every register, so they do
not establish an argument order or dimensions.

This step therefore executes only the observed source and destination object
addresses: x1 is the source GPU address and x2 is the destination texture for
`CopyBufferToTexture`. For `CopyTextureToTexture`, treating x1 as the source
and x2 as the destination is novena's assumption, not an observation. It copies the complete base-level opaque byte block,
using four bytes per texel, novena's existing format-independent storage
choice. The unresolved pointer arguments remain raw in the recorded command.

Further observation must show, for each copy call, the register associated
with each pointer, and a pointee dump with offsets labelled. The shape files
need samples varying one rectangle, mip level, layer, source offset, and
destination offset at a time, with the words at +0x00, +0x04, +0x08 and onward
classified. A format census must also relate the texture format register to
the byte count and row stride before any format-specific conversion is added.
