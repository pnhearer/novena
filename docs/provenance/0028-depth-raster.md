# 0028: depth, stencil and rasterizer state

- Date: 2026-10-08
- Author: Khargoosh
- Covers: bounded depth and stencil attachments, rasterizer state and headless readback

## Sources

[Signatures 0003](../signatures/0003-objects.md) establishes enable flags,
depth-function values, the likely stencil-function and operation arguments,
cull-face values and polygon-mode values.
[Signatures 0002](../signatures/0002-command-buffer.md) establishes an optional
depth target and records the open polygon-offset and clear shapes.
[Signatures 0010](../signatures/0010-remaining-command-state.md) establishes
stencil command selectors and values and the three observed float-register
words for polygon offset. The offset words do not vary, so their order remains
open. No packed state object or observed enum meaning was inferred.

Public Vulkan sources define the host operations:

- [Depth and stencil state](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineDepthStencilStateCreateInfo.html)
  defines enable flags, depth comparison and separate front and back stencil state.
- [Comparison operators](https://docs.vulkan.org/refpages/latest/refpages/source/VkCompareOp.html)
  and [stencil operations](https://docs.vulkan.org/refpages/latest/refpages/source/VkStencilOp.html)
  define the public enum table order.
- [Front face](https://docs.vulkan.org/refpages/latest/refpages/source/VkFrontFace.html)
  defines winding in framebuffer coordinates.
- [Stencil state](https://docs.vulkan.org/refpages/latest/refpages/source/VkStencilOpState.html)
  defines fail, pass and depth fail operations, compare mask, write mask and reference.
- [Rasterization state](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineRasterizationStateCreateInfo.html)
  defines culling, winding, polygon mode and bias factors, with optional feature requirements.
- [Buffer and image copies](https://docs.vulkan.org/spec/latest/chapters/copies.html)
  defines separate depth and stencil aspects and their buffer representations.

Synthetic programs reuse the original public Mesa and envytools encodings
recorded in [0027](0027-drawing.md). The separate translator is used only through
its public header-prefixed translation signature. No translator implementation
supplies guest-state facts. The selected checkout remains read-only.

## Choices and hypotheses

`DepthRasterContract` supplies all raw token meanings. The test uses synthetic
tokens, not measured values. Comparison and operation table positions follow
Vulkan enumerations. Selectors identify front, back and both faces. No token
table is active without host opt-in.

The draw attachment uses D32_SFLOAT_S8_UINT with load/store for both aspects.
Canonical storage is a depth-float plane followed by a stencil-byte plane,
with no row padding. This is an explicit host format. Each aspect has its own
transfer region, and the pool bounds include both planes. Barriers cover early
and late fragment tests. Submission fences protect temporary views and the
framebuffer. Legacy D32 clears retain their earlier behavior.

Stencil-function order is hypothesized as face, compare, reference, value mask.
Stencil-operation order is hypothesized as face, stencil fail, depth fail, pass.
Command write mask, comparison mask and reference override bound values by face.
Bindings retain setter snapshots and face-specific setters preserve the other
face. Every recording starts with empty command state. Enabled stencil requires
explicit functions, operations and write masks for both faces.

Front face is a host winding choice because no recorded setter is established.
Polygon offset requires separate opt-in to d0 slope, d1 constant, d2 clamp.
The recorder preserves full register words and the executor rejects values that
are not finite f32 words. Optional non-solid and bias-clamp device features are
enabled when supported and checked before creation.

The graphics key now includes depth, stencil, masks, reference, attachment
presence, culling, winding, mode and bias. The persistent graphics namespace
advances to interface 2. Explicit retry takes the same interpreted state, with public Vulkan enum values
validated before cache lookup.
Queued pipelines retain the existing skip policy.

## Confirming experiments

The headless `depth_stencil_and_raster_pixels` test records the public call
shapes, translates original synthetic vertex and fragment programs, validates
their SPIR-V and reads the rendered pixels through presentation. It also checks
canonical color bytes and the depth and stencil planes.

It exercises both overlapping-depth draw orders, depth writes disabled, all
comparison tokens, masked stencil replacement, a second draw selected by the
stencil mask, every stencil operation including clamp and wrap boundaries, and
distinct stencil fail and depth fail paths. Separate dynamic references check both faces, and a value-mask comparison
ignores high reference and stored bits without changing the write mask.
Culling covers both vertex orders,
all cull selections and both host windings. Line and point modes must leave the
interior clear while producing pixels elsewhere. Bias tests inspect constant bias, slope bias and the clamp in pixel and depth readback.
Unit checks cover invalid public retry state, separate face updates, missing write masks, unknown and
ambiguous tokens, oversized stencil values, nonfinite bias and full-width offset recording.

These tests confirm the implemented host contract and distinguish its argument
roles. They do not confirm guest enum meanings, stencil argument order, polygon
offset order, winding, attachment format or arena layout. Guest confirmation
requires isolated changes to observed calls and measured depth, stencil and
pixel results. ClearDepthStencil remains an open signature.

The combined `indexed_strip_uniform_depth_pixels` proof retains strip topology,
a normalized zero-stride second stream and stage-local banks in both descriptor
modes. It adds an indexed draw with an advanced address and negative base
vertex, plus depth testing and writes. Color and depth readback confirm the
combined path. A short second-stream binding is rejected before submission.

## Verification

All checks below returned exit code zero on 2026-10-08:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/drawing.rs
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy --manifest-path target/shadowbox-global-memory/depth_stencil_and_raster_pixels/Cargo.toml --target-dir target/shadowbox-global-memory/build --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked -- --include-ignored --nocapture --test-threads=1
```

The default suite passed 42 workspace tests. The all-feature suite passed 73
workspace tests with zero failures and zero ignored tests. The translator was
selected through `NOVENA_SHADOWBOX_PATH`. Both drawing proofs executed without
a skip, including the earlier cache restart and recovery proof. All 156
translated memory cases matched. The depth proof checked presented pixels,
canonical color bytes, depth floats and stencil bytes.

The generated translator test package also passed Clippy with warnings denied.
Formatting covers the separately compiled drawing source. Temporary shaders,
Cargo cache, build output and test cache files stay in the ignored build tree.
These checks prove the bounded host contract. They do not establish guest mappings.
