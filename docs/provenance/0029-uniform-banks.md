# 0029: uniform banks in translated draws

- Date: 2026-10-08
- Author: Khargoosh
- Covers: retained uniform bindings, stage-local constant banks, arena descriptors,
  storage fallback and the headless color proof

## Sources and what was learned

Repository signatures 0002 and 0010 record `BindUniformBuffer` arguments as
command object, stage, index, wide GPU-shaped location and extent. Observed
stages are 0, 1 and 5, indices are 0 through 2, and extents range from 0x10 to
0xaa80. No source establishes stage meanings or index-to-bank relationships.
This change does not add a new observation of an existing program.

The selected Shadowbox source, revision
`2ee031ad7e62a76296d1465c5438c8aeb3173803`, supplies the translator interface.
Its `crates/shadowbox/src/emitter.rs` collects referenced constant banks and
emits each as `Block { float4[4096] }`, member offset zero, array stride 16,
`Uniform` storage, descriptor set zero and binding equal to the bank number.
Its source lowering divides byte offsets by 16 and selects component
`offset / 4 % 4`. The older `docs/provenance/0007-constant-buffers.md` records
the bank-number convention and public sources. The current emitter supplies
the exact array type and extent. No translator code was copied.

The synthetic MOV constant-source instruction follows Mesa revision
`7b76f8646e387c17022f8586d224738e310b5bfc`,
`src/nouveau/compiler/nak/sm50.rs`, `OpMov::encode` and `set_src_cb`.
Opcode 0x4c98 selects the constant form. Bits 20 through 33 select a four-byte
word offset; bits 34 through 38 select the bank. Bits 39 through 42 select all
four lanes with value 15. Vertex input, position output, header fields, EXIT,
NOP and bundle construction reuse the public encoding facts in provenance
0027. The test has original shaders, vertex bytes and colors.

Public host rules come from:

- [Buffer descriptors](https://docs.vulkan.org/refpages/latest/refpages/source/VkDescriptorBufferInfo.html),
  which define offsets relative to a buffer and byte ranges.
- [Descriptor updates](https://docs.vulkan.org/refpages/latest/refpages/source/VkWriteDescriptorSet.html),
  which require uniform and storage range limits and offset alignments.
- [Device limits](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceLimits.html),
  which define uniform and storage ranges, alignments and descriptor counts.
- [Shader interfaces](https://docs.vulkan.org/spec/latest/chapters/interfaces.html),
  which define descriptor sets, bindings, block layouts and storage classes.
- [Descriptor pools](https://docs.vulkan.org/refpages/latest/refpages/source/VkDescriptorPoolCreateInfo.html)
  and [descriptor binding](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdBindDescriptorSets.html),
  which define allocation and command use.

Only the permitted repository observations, the selected translator interface,
Mesa encoding facts and public host documentation informed this change.

## Implementation choices

A separate Rust host contract maps recorded stage/index pairs to vertex or
fragment banks. The test maps stage 0/index 1 to vertex bank 2 and stage 1/index
0 to fragment bank 2. These values are explicit experiment choices. They are
not proposed guest mappings. Mapping sources and targets must be unique.
The default contract is empty.

Each translated stage is inspected for the exact constant-bank block shape.
Only set-zero banks numbered below 32 and singleton uniform descriptors enter
this path. Other resource types remain unsupported. Vertex set zero remains
unchanged; fragment bank declarations become set one. Identical bank numbers
in different stages can therefore use independent ranges.

Pool resolution selects the canonical arena buffer, its pool offset plus the
relative address offset, and the exact recorded byte extent. Extents must be
nonzero multiples of 16 no larger than 64 KiB. Invalid pool ranges and device
misalignment are errors. No rounding, copying, padding or implicit neighboring
bytes enter a descriptor. Shader reads must stay within the bound extent.
Static and indirect out-of-range shader-read semantics remain open.

The backend uses uniform descriptors when the device can bind a full bank.
A maximum uniform range below 64 KiB selects storage descriptors for all banks
in the pipeline. The host can force this path. The rewrite changes Uniform
pointers and variables to StorageBuffer and adds NonWritable decorations.
It preserves block shape, loads and byte indexing. It requires SPIR-V 1.3.
Storage descriptor limits and alignment apply on that path. Device descriptor
counts and layout support are checked before creation.

The storage choice is part of the graphics key; buffer addresses and contents
are absent. Graphics cache interface version 2 separates old descriptor layouts.
The per-draw descriptor pool owns two stage sets and survives until synchronous
submission completion. The existing arena barrier orders uploads and prior
writes before uniform or storage reads. The attachment fence protects sets,
pipeline, pools and target transfers.

## Experiment

The vertex shader loads bank 2 at byte zero into position W. The fragment shader
loads bank 2 at bytes 16 through 28 into its four output components. Decoy data
at byte zero catches lost shader offsets. The ranges have nonzero arena offsets
aligned to the device's uniform and storage requirements.

Two fragment bindings change green from 0.25 to 0.75 with identical retained
shader words. Covered pixels match `[255, 64, 128, 255]` and
`[255, 191, 128, 255]`; uncovered pixels retain blue. The test checks presentation
readback and canonical pool storage. A recording containing both draws checks
that rebinding reaches the later draw. Pipeline misses stay at one per descriptor
mode, and the translator runs exactly once per stage.

The same proof forces the storage path on the available device. It also binds
a full 64 KiB range, rejects misalignment, zero and partial-vector extents,
overlarge and pool-crossing extents, and rejects missing or unmapped bindings.
Duplicate mappings fail without replacing the active contract. Reflection tests
reject changed array lengths, stride, member offset and scalar width; lowering
checks preserve the bank number and layout while separating sets and storage.

Guest stage meanings, bank mappings, inheritance across recordings and behavior
of shader reads beyond the bound extent remain unobserved. This proof closes
the bounded host uniform-data path, not those guest questions.

## Combined drawing experiment

`uniform_banks_with_strip_and_normalized_attribute` uses original source shaders
with matching float varyings and the same constant-bank block shape. A synthetic
strip token selects four controlled positions. A normalized four-byte second
attribute comes from a separate zero-stride stream with a one-byte offset.

Both uniform and forced storage modes produce the expected rectangle and colors.
Changing only the fragment bank range changes green from 32 to 96 while red and
blue stay 255 and 32. Readback checks both canonical bytes and presentation.
A one-byte-short attribute range fails before pipeline use. Ready draws reuse one
pipeline per selected descriptor mode. Cold recordings queue compilation; fresh
recordings follow worker completion. This experiment combines the host topology,
vertex-format and bank contracts without proposing guest mappings.

## Verification

These commands returned exit code zero:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/drawing.rs
cargo test --workspace --locked
cargo test --workspace --all-features --locked -- --include-ignored --nocapture --test-threads=1
cargo test --features shadowbox --test shadowbox_drawing uniform_banks_with_strip_and_normalized_attribute -- --ignored --nocapture
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy --manifest-path target/shadowbox-global-memory/first_draw_executes_translated_triangle/Cargo.toml --target-dir target/shadowbox-global-memory/build --all-targets -- -D warnings
```

Default tests passed 41 workspace tests. The full feature run passed 56 unit
tests and 19 integration tests, with no ignored tests or optional skips.
The translator was selected. All five drawing proofs ran on the headless
hardware path, including the strip and normalized-attribute bank experiment.
Both uniform and forced read-only storage descriptors produced expected pixels.
The full run also passed topology and format matrices, presentation,
persistent-cache restart and recovery, compute pipelines, and all 156
translated global-memory cases. Reflection checks validated both rewritten
descriptor modes with `spirv-val`.

Clippy passed the generated drawing package with all targets and warnings
denied. Test output, Cargo cache, disposable modules and driver caches remained
under the checkout's ignored `target` directory.
