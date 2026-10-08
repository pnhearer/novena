# 0027: first draw

- Date: 2026-10-08
- Author: Khargoosh
- Covers: recorded draw state, graphics pipelines, color attachments, the
  translated triangle proof, and the drawing and pipeline designs

## Facts and sources

The call shapes come from signatures 0002, 0003, 0004, and 0009. They establish
non-indexed first/count arguments, vertex buffer address/size, counted vertex
state, stride, attribute offset, stream index, and enable flags.
Signature 0004 supports a direct stream-state object at count one.
It does not establish the attribute-state array layout. The experiment uses
a direct attribute object as an explicit implementation choice.
No new guest enum conversion or object layout was inferred.

Pool resolution and the flat arena follow provenance 0016, 0022, and 0023.
Shader capture, normalized headers, retained translation, and the existing
count-two reader assumption follow 0017, 0021, and 0025.
The first draw test uses the separate translator's header-prefixed interface
already described in 0025. The selected translator revision is
2ee031ad7e62a76296d1465c5438c8aeb3173803. That checkout remained read-only.
Only its public interface contract was used. No other implementation informed
guest behavior.

The original synthetic shaders use public Mesa and envytools encoding facts.
Mesa revision 7b76f8646e387c17022f8586d224738e310b5bfc supplies:

- `src/nouveau/compiler/nak/sm50.rs`, `OpALd` and `OpASt`: opcode, register,
  address, output, and component-count fields. The test loads four components
  at generic address 0x80 into registers zero through three and stores them
  at position address 0x70.
- `src/nouveau/compiler/nak_private.h`: position addresses 0x70 through 0x7c
  and the generic attribute base 0x80.
- `src/nouveau/compiler/nak/sph.rs` and the public shader-header definitions
  in `src/nouveau/headers/nvidia/classes/cla097sph.h`: stage bits 10 through 13,
  vertex input mask starting at bit 192, position output mask at bit 400,
  and fragment color output mask at bit 576.

The test constructs an 80-byte version-three vertex or fragment header.
It declares one generic float4 input and position output for the vertex stage,
and four color components at target zero for the fragment stage.
The fragment writes constants through registers zero through three.
MOV32I, EXIT, NOP, and control words use the encoding facts already recorded in
0023 from envytools `envydis/gm107.c`, revision
f102b82381f3f11cee113d16374c87091db039d9. No upstream implementation was copied.
No observed program's vertices, shader instructions, or output enter the test.

The host implementation follows public Vulkan documentation:

- [Graphics pipeline creation](https://docs.vulkan.org/refpages/latest/refpages/source/VkGraphicsPipelineCreateInfo.html)
  defines the paired shader stages and fixed or dynamic state.
- [Vertex attributes](https://docs.vulkan.org/refpages/latest/refpages/source/VkVertexInputAttributeDescription.html)
  defines location, binding, format, offset, and device limits.
- [Non-indexed drawing](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdDraw.html)
  defines first vertex, count, and instance arguments.
- [Viewport](https://docs.vulkan.org/refpages/latest/refpages/source/VkViewport.html)
  defines the explicit coordinate choice and viewport limits.
- [Attachments](https://docs.vulkan.org/refpages/latest/refpages/source/VkAttachmentDescription.html)
  defines load/store behavior, format, samples, and initial/final layouts.
- [Image barriers](https://docs.vulkan.org/refpages/latest/refpages/source/VkImageMemoryBarrier.html)
  defines visibility and layout transitions.
- The public SPIR-V grammar linked in 0025 supplies execution models, interface
  types, variables, and decorations. The inspector does not replace validation.

## Implementation choices

`FirstDrawContract` is a Rust host opt-in. It identifies raw tokens for triangle
list, float4 input, no culling, RGBA8, a 2D target, and identity swizzle.
The test chooses synthetic tokens 0xf001 through 0xf005. Those values are not
proposed guest enums. A submitted draw without a contract is unsupported.
The contract also fixes Vulkan pixel coordinates, fill, one sample, all color
channels, and zero-through-one depth range. Recorded state must explicitly
turn off blend, depth test, depth write, and stencil test.

State bindings capture owned side-table settings at recording time.
The executor resolves the vertex range through a registered pool and includes
first vertex, stride, and attribute offset in its last-fetch bounds check.
Stride and offset must be four-byte aligned. Viewport and scissor origins must
be zero. Sizes must fit the target and Vulkan limits. Unsupported state and
unknown recorded commands stop execution with `Unimplemented`.
Each recording starts with empty draw state. Cross-recording inheritance is open.

Exactly two retained translations belong to the bound program. Execution
models pair vertex and fragment stages independently of record order or mask.
The vertex input location comes from a single float4 declaration. The fragment
output must be float4 at location zero. Descriptors, user varyings, extra stages,
extra attributes, component decorations, and fragment builtin outputs are
unsupported. Emitted specialization defaults remain unchanged.

The backend cache compares full translated stage words, stride, and offset.
All other supported pipeline state is fixed. Guest addresses and dynamic
viewport/scissor values are absent from the key. A hit precedes reflection and
Vulkan creation. A miss creates a load/store render pass, an empty descriptor
layout with the published global-delta push range, and a graphics pipeline
through a guarded driver cache shared by the worker machinery. Failed requests
require explicit retry. Temporary shader
modules and partial Vulkan objects are destroyed on every creation path.

The arena buffer adds vertex-buffer usage. Images add color-attachment usage.
Rendering loads canonical arena bytes into an optimal image, references vertex
bytes directly from the arena, and stores the target back into the arena.
Barriers order host uploads, vertex reads, transfer operations, and color writes.
Fence completion precedes destruction of temporary views and framebuffers.
If a later command fails, earlier completed operations still download their
canonical bytes. These choices do not establish target synchronization rules.

## Experiment and open questions

The headless test constructs original vertex and fragment programs, validates
each translation with `spirv-val --target-env vulkan1.2`, and records the triangle
through command-buffer handlers and queue submission. It uses a 64 by 64 color
target, padded stride 32, attribute offset 8, first vertex 1, and a nonzero vertex
pool offset. The fragment record precedes the vertex record. The stage mask is
uninterpreted. The shader has no texture or uniform resource.

The test presents through the existing GPU offscreen path and checks covered
and uncovered pixels, including interior and exterior samples across the image.
It checks both presented readback and canonical arena bytes.
A cold request and three subsequent draws produce one miss and three hits
without additional translation.
A changed fragment constant produces a second pipeline and different pixels.
A setter after binding cannot change the earlier state snapshot.
Short vertex ranges, an unknown primitive, and enabled blend are rejected.
An unsupported draw after a successful changed-content draw returns an error
while preserving the successful draw's pixels.

This proves the bounded host-contract draw. It does not establish real guest
topology, attribute formats or attribute-array layout, cull values, color formats,
swizzle, coordinate origin, shader-record stride, stage masks, or resource
bindings. Complete limits and remaining draw work are in
[Drawing support and open questions](../design/drawing.md).
General feature negotiation, C configuration, state inheritance and asynchronous
submissions remain open. The integration below adds persistent graphics caches.

## Original verification

All commands below completed with exit code zero on 2026-10-08:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/drawing.rs
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --manifest-path target/shadowbox-global-memory/first_draw_executes_translated_triangle/Cargo.toml --target-dir target/shadowbox-global-memory/build --all-targets -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked -- --include-ignored --nocapture --test-threads=1
```

The selected translator was configured for the all-feature run. The GPU proof
did not skip. It ran on a Vulkan device with an open driver. Default tests
passed 35 tests. The all-feature run passed 54 workspace tests, including the translated triangle, compute-cache proof,
headless presentation, and the 156 global-memory cases.
Build, Cargo cache, temporary shader files, and driver cache output remained
inside the working checkout's ignored `target` directory.

## Cache and retained-state integration

The graphics builder now uses the bounded owned-request service and guarded
driver-cache implementation from provenance 0026. Separate compute and graphics
interface domains prevent cross-service disk namespace reuse. The graphics
key still compares complete translated stages and vertex input. The host
supplies its private directory and identity through
`Instance::set_graphics_pipeline_cache`. No default filesystem location is used.

Graphics workers reflect and create pipelines, then save complete driver
snapshots. Queue execution requests and polls without compilation or file I/O.
Queued, compiling and queue-full results skip that draw while other commands
continue. Failed requests report an unsupported draw until explicitly retried.
This retains the host policy recorded in 0026 and does not assert equivalent
guest output.

The public Vulkan sources for this integration are
[graphics creation](https://docs.vulkan.org/refpages/latest/refpages/source/vkCreateGraphicsPipelines.html),
[cache retrieval](https://docs.vulkan.org/refpages/latest/refpages/source/vkGetPipelineCacheData.html)
and [cache creation](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineCacheCreateInfo.html).
They define use of a driver cache during graphics creation and retrieval of data
for later creation. The checked disk format and synchronization remain the
implementation choices and original experiments in 0026.

The executor consumes the existing `StateCommand` vertex-buffer and color,
depth-stencil and polygon snapshots. It retains all 17 state-only handlers and
their tests. Count-one vertex stream and attribute records add the bounded
experiment without inferring packed layouts. Unhandled recorded state rejects
a dependent draw while clears and copies remain executable. The census test
continues to account for all 26 original gaps, including the two vertex bindings
now handled by the experiment.

The extended triangle test waits for a cold worker result outside submission,
then executes ready draws and checks exact presentation and arena pixels.
It checks changed shader content, state snapshots, rejection paths and download
of a completed draw before a later error. It reopens the graphics disk cache,
checks that validated driver data was supplied, damages that blob, recompiles,
and reopens the repair in a fresh subprocess. Reopening does not translate
again in queue execution. Driver-load statistics prove accepted initial data,
not driver compilation hit rates or speedups.


## Integration verification

The integrated default workspace suite passed 41 tests. The all-feature suite
passed 68 workspace tests, with zero failures, zero ignored tests and no
translator skips. All 156 translated memory cases matched. The graphics
triangle ran in the initial process and a fresh subprocess, including disk
loading, recovery and exact pixel checks. The extended disk namespace test
also passed, keeping compute and graphics records separate.

These commands returned exit code zero:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/drawing.rs crates/novena/tests/shadowbox/global_memory.rs
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked -- --include-ignored --nocapture --test-threads=1
```

All four generated translator test packages passed Clippy with all targets,
locked dependencies and warnings denied. The Vulkan-feature workspace suite
also passed with both driver-selection variables pointing at an absent file,
checking CPU fallback. Formatting covered both generated test source files.

Every integration commit diff passed the check for local paths, checkout names,
excluded product names in prose, em dashes and attribution trailers. Exact
upstream source paths remain evidence. Build output, Cargo cache and disposable
graphics cache files stayed under the checkout's ignored `target` directory.
