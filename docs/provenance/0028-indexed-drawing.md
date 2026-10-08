# 0028: indexed drawing

- Date: 2026-10-08
- Author: Khargoosh
- Covers: indexed command execution, canonical arena index reads, and original
  headless pixel tests

## Facts and sources

[Signatures 0002](../signatures/0002-command-buffer.md#drawing-and-clearing)
and [0009](../signatures/0009-draw-state.md) establish the argument order
`commands, primitive, indexType, count, indices, baseVertex`. Observed index
types are 1 and 2, counts are 3 and 6, the index address is wide, and base vertex
is zero. They do not establish element widths, signedness, byte order, count
semantics, topology or restart. No separate first-index argument or index
buffer binding is established. This work adds no guest observation.

Pool resolution and the canonical flat arena use the project facts and
implementation choices recorded in provenance 0016, 0022 and 0023.
The vertex state, translated stage pairing, graphics workers, attachment path,
and original Mesa and envytools shader encoding sources are those in
[provenance 0027](0027-drawing.md). No additional shader encodings or translator
implementation facts were used.

The host execution follows public Vulkan documentation:

- [Indexed drawing](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdDrawIndexed.html)
  defines consecutive unsigned index elements, first-index selection and signed
  vertex offset addition.
- [Index buffer binding](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdBindIndexBuffer.html)
  defines the buffer byte offset, element alignment and index-buffer usage.
- [Index types](https://docs.vulkan.org/refpages/latest/refpages/source/VkIndexType.html)
  defines unsigned 16-bit and 32-bit types used by this experiment.

No upstream implementation was copied. Tests use only original synthetic
vertices, index arrays, shaders and colors.

## Hypotheses and implementation choices

`FirstDrawContract` adds distinct host-selected `index_u16` and `index_u32`
tokens. The tests use 0xf006 and 0xf007. They do not propose guest enum values
or assign widths to observed values 1 and 2. Both types reuse the host-selected topology pipeline with restart disabled.

The experiment interprets count as the number of tightly packed little-endian
index elements, and the low base-vertex word as a signed 32-bit integer.
It adds that value to each unsigned index before selecting the vertex.
These remain guest hypotheses. Synthetic tests confirm the host behavior,
not the hypotheses about guest behavior.

The supplied index address resolves through the registered pool to the arena
buffer. Binding uses that exact byte offset, with Vulkan first index zero.
The call carries no separate first index. Selecting a later element therefore
requires advancing the address by a whole number of element widths.
The arena adds index-buffer usage. Existing memory barriers and submission
fences cover index reads along with vertex reads and attachment transfers.

Before submitting a nonempty draw, the executor checks the entire index range
against its pool and alignment against its width. It reads canonical arena
bytes in chunks of at most 64 KiB, then checks every active attribute using its host-selected format width
against the recorded vertex-buffer size, stride and attribute offset.
The checks retain support for shared and separate streams and zero stride.
This avoids a count-sized host allocation and uses completed GPU results
rather than stale host callback bytes. Negative or overflowing vertex results
are rejected instead of wrapping. A zero count does not resolve the index
address or read index elements, after common state and vertex-binding checks.
Unknown or ambiguous index tokens remain unsupported.

## Confirming tests

`indexed_draw_executes_translated_triangle` records commands through the public
handlers and submits them to a headless Vulkan device. It reuses the original
header-prefixed vertex and fragment programs from 0027. Translation runs
through the selected translator's public interface and each result passes
`spirv-val --target-env vulkan1.2`.

For both index widths, the test draws a triangle using a reordered index array
with base vertex zero, positive one and negative one. Each array selects the
same three vertices only after the base is applied. Vertex binding has a
nonzero pool offset, stride 32 and attribute offset 8. Index binding also has a
nonzero pool offset. The 16-bit selected address is two-byte aligned but not
four-byte aligned. A poison prefix precedes that address and a poison suffix
lies outside count three. Exact covered and uncovered pixels are checked in
presentation readback and canonical pool storage. Additional pixel cases use
16-bit elements near 65535 and 32-bit elements above 65535 with compensating
negative base vertex. They check unsigned extension and the upper index bits.

Count six selects a second triangle with separate pixel coverage. Both
readback paths must contain that second triangle. The test also rejects a
pool-crossing index range, misaligned index address, unknown token, unresolved
address, short vertex range, negative selected vertex and excessive base
vertex. Count zero accepts an unused null index address and leaves the clear
color intact. Both widths and all base values share one pipeline and reuse
retained translations. Unit tests check element decoding, a 32-bit element
above 65535, signed addition and overflow rejection.

The tests confirm this bounded implementation. Guest index widths, byte order,
signedness, count semantics, topology and restart still need controlled
observations of the kind proposed in the drawing design.

## Verification

The default workspace suite passed 41 tests. The all-feature suite passed 70
workspace tests, including both headless drawing proofs, with no failures or
translator skips. All 156 translated memory cases matched. The indexed proof
also passed after adding the large unsigned-element cases. Formatting and
Clippy passed for both workspace feature configurations and for the generated
graphics test package with warnings denied.

These commands completed with exit code zero:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/drawing.rs
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy --manifest-path target/shadowbox-global-memory/indexed_draw_executes_translated_triangle/Cargo.toml --target-dir target/shadowbox-global-memory/build --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked -- --include-ignored --nocapture --test-threads=1
cargo test --features shadowbox --test shadowbox_drawing indexed_draw_executes_translated_triangle -- --ignored --nocapture
```

`NOVENA_SHADOWBOX_PATH` was set to the selected translator crate for the GPU
checks. The proof ran on an open Vulkan driver. Generated packages, Cargo
cache, temporary files and driver cache output stayed under ignored `target`.
