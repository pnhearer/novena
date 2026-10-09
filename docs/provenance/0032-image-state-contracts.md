# 0032: image and sampler state contracts

- Date: 2026-10-09
- Author: Khargoosh
- Covers: format, target, storage, component selection and sampler interpretation

## Evidence boundary

[Object signatures](../signatures/0003-objects.md#textures) establish the
positions of texture builder fields. The
[startup shapes](../shapes/0001-program-a-startup.txt) record their numeric
arguments. [Sampler signatures](../signatures/0001-setup.md#samplers) describe
the setters and their confidence. The
[startup census](../census/0001-program-a-startup.txt) establishes call counts.
These records do not establish any numeric guest format, target, storage,
component or sampler enum correspondence with Vulkan.

The MIT-licensed
[open-source GPU driver library format implementation](https://github.com/chaotic-cx/mesa-mirror/blob/mesa-24.0.0/src/nouveau/nil/nil_format.c)
separates format interpretation, component sources, numeric types and supported
uses. Its related image implementation informs the existing tiled packing.
Neither file establishes the guest builder enum numbering. No code was copied.

The [Vulkan format specification](https://docs.vulkan.org/spec/latest/chapters/formats.html),
[component selection rules](https://docs.vulkan.org/refpages/latest/refpages/source/VkComponentSwizzle.html),
[image view requirements](https://docs.vulkan.org/refpages/latest/refpages/source/VkImageViewCreateInfo.html)
and [sampler specification](https://docs.vulkan.org/spec/latest/chapters/samplers.html)
establish host semantics and device checks. Test images, shaders and sampler
fixtures are original.

A complete numeric guest enum table cannot be reconstructed from these sources.
The recorded format list is truncated, and the records contain no
format-to-pixel correlation. There are zero established numeric guest format
mappings. Exact host sampling does not change that count.

## Recorded tokens

"Established" below means that the token was recorded in the stated field.
Every proposed host meaning remains a hypothesis and requires a host rule.

| Guest field | Established recorded values | Host interpretation |
| --- | --- | --- |
| Format | 0x0b, 0x0c, 0x16, 0x25, 0x27, 0x9a; observed extrema 1 and 0x9c | Hypothesis, unresolved for every value |
| Target | 1 | Hypothesis, host must select a view shape |
| Target | 2 | Hypothesis, host must select a view shape |
| Target | 4 | Hypothesis, host must select a view shape |
| Target | 8 | Hypothesis, host must select a view shape |
| Target | 0x0a | Hypothesis, host must select a view shape |
| Flags | 0, 4, 8, 9, 0x0c, 0x20 | Hypothesis, no recorded tiling selector |
| Component selector | 2, 3, 4, 5 | Hypothesis, no established component correspondence |
| Depth/stencil mode | 0, 1 | Hypothesis, no established aspect correspondence |
| Minification | 2, 3, 5 | Hypothesis, texel and mip filter pair required |
| Magnification | 0, 1 | Hypothesis, nearest or linear choice required |
| Wrap | 1, 5, 7 | Hypothesis, address mode required independently on each axis |
| Compare | Mode 1, function 2 in one call | Hypothesis, enable and comparison function required |
| Anisotropy | Float values 1, 4, 8 | Float argument established; enabling interpretation is a host hypothesis |
| Border | Readable pointer | Color contents and interpretation unresolved |
| LOD bias and bounds | No varying values | Float order remains a host hypothesis |

The format extrema do not establish that every intervening integer is an enum.
No missing guest value is assigned from the order of a public driver table.

## Host format catalogue

Every row establishes Vulkan host semantics. The guest association for every
row has status Hypothesis, unresolved. A host supplies the guest token and
rationale before use. Integer formats require an integer sampled image type;
normalized, floating and sRGB formats return floating components. Depth
formats use a depth aspect; stencil uses a stencil aspect. Missing color
components follow Vulkan's zero and one rules. Transfers retain encoded bytes
and do not decode compressed blocks.

The table covers the 102 single-aspect formats accepted by format_block.
The transfer_formats function enumerates the same set for the reusable GPU
check. The test device supports 73 of them. It reports X8_D24_UNORM_PACK32 and
all 28 ASTC variants unavailable. Creation rejects unavailable formats.

| Host format | Block extent | Bytes per block | Host semantics | Native check |
| --- | --- | ---: | --- | --- |
| R8_UNORM | 1 x 1 | 1 | Established | Exact sampling and readback |
| R8_SNORM | 1 x 1 | 1 | Established | Exact sampling and readback |
| R8_UINT | 1 x 1 | 1 | Established | Exact sampling and readback |
| R8_SINT | 1 x 1 | 1 | Established | Exact sampling and readback |
| R8_SRGB | 1 x 1 | 1 | Established | Exact sampling and readback |
| S8_UINT | 1 x 1 | 1 | Established | Exact sampling and readback |
| R8G8_UNORM | 1 x 1 | 2 | Established | Exact sampling and readback |
| R8G8_SNORM | 1 x 1 | 2 | Established | Exact sampling and readback |
| R8G8_UINT | 1 x 1 | 2 | Established | Exact sampling and readback |
| R8G8_SINT | 1 x 1 | 2 | Established | Exact sampling and readback |
| R8G8_SRGB | 1 x 1 | 2 | Established | Exact sampling and readback |
| R16_UNORM | 1 x 1 | 2 | Established | Exact sampling and readback |
| R16_SNORM | 1 x 1 | 2 | Established | Exact sampling and readback |
| R16_UINT | 1 x 1 | 2 | Established | Exact sampling and readback |
| R16_SINT | 1 x 1 | 2 | Established | Exact sampling and readback |
| R16_SFLOAT | 1 x 1 | 2 | Established | Exact sampling and readback |
| D16_UNORM | 1 x 1 | 2 | Established | Exact sampling and readback |
| R4G4B4A4_UNORM_PACK16 | 1 x 1 | 2 | Established | Exact sampling and readback |
| B4G4R4A4_UNORM_PACK16 | 1 x 1 | 2 | Established | Exact sampling and readback |
| R5G6B5_UNORM_PACK16 | 1 x 1 | 2 | Established | Exact sampling and readback |
| B5G6R5_UNORM_PACK16 | 1 x 1 | 2 | Established | Exact sampling and readback |
| R5G5B5A1_UNORM_PACK16 | 1 x 1 | 2 | Established | Exact sampling and readback |
| B5G5R5A1_UNORM_PACK16 | 1 x 1 | 2 | Established | Exact sampling and readback |
| A1R5G5B5_UNORM_PACK16 | 1 x 1 | 2 | Established | Exact sampling and readback |
| R8G8B8A8_UNORM | 1 x 1 | 4 | Established | Exact sampling and readback |
| R8G8B8A8_SNORM | 1 x 1 | 4 | Established | Exact sampling and readback |
| R8G8B8A8_UINT | 1 x 1 | 4 | Established | Exact sampling and readback |
| R8G8B8A8_SINT | 1 x 1 | 4 | Established | Exact sampling and readback |
| R8G8B8A8_SRGB | 1 x 1 | 4 | Established | Exact sampling and readback |
| B8G8R8A8_UNORM | 1 x 1 | 4 | Established | Exact sampling and readback |
| B8G8R8A8_SRGB | 1 x 1 | 4 | Established | Exact sampling and readback |
| R16G16_UNORM | 1 x 1 | 4 | Established | Exact sampling and readback |
| R16G16_SNORM | 1 x 1 | 4 | Established | Exact sampling and readback |
| R16G16_UINT | 1 x 1 | 4 | Established | Exact sampling and readback |
| R16G16_SINT | 1 x 1 | 4 | Established | Exact sampling and readback |
| R16G16_SFLOAT | 1 x 1 | 4 | Established | Exact sampling and readback |
| R32_UINT | 1 x 1 | 4 | Established | Exact sampling and readback |
| R32_SINT | 1 x 1 | 4 | Established | Exact sampling and readback |
| R32_SFLOAT | 1 x 1 | 4 | Established | Exact sampling and readback |
| D32_SFLOAT | 1 x 1 | 4 | Established | Exact sampling and readback |
| A2B10G10R10_UNORM_PACK32 | 1 x 1 | 4 | Established | Exact sampling and readback |
| A2R10G10B10_UNORM_PACK32 | 1 x 1 | 4 | Established | Exact sampling and readback |
| A2R10G10B10_UINT_PACK32 | 1 x 1 | 4 | Established | Exact sampling and readback |
| A2B10G10R10_UINT_PACK32 | 1 x 1 | 4 | Established | Exact sampling and readback |
| B10G11R11_UFLOAT_PACK32 | 1 x 1 | 4 | Established | Exact sampling and readback |
| E5B9G9R9_UFLOAT_PACK32 | 1 x 1 | 4 | Established | Exact sampling and readback |
| X8_D24_UNORM_PACK32 | 1 x 1 | 4 | Established | Unavailable |
| R16G16B16A16_UNORM | 1 x 1 | 8 | Established | Exact sampling and readback |
| R16G16B16A16_SNORM | 1 x 1 | 8 | Established | Exact sampling and readback |
| R16G16B16A16_UINT | 1 x 1 | 8 | Established | Exact sampling and readback |
| R16G16B16A16_SINT | 1 x 1 | 8 | Established | Exact sampling and readback |
| R16G16B16A16_SFLOAT | 1 x 1 | 8 | Established | Exact sampling and readback |
| R32G32_UINT | 1 x 1 | 8 | Established | Exact sampling and readback |
| R32G32_SINT | 1 x 1 | 8 | Established | Exact sampling and readback |
| R32G32_SFLOAT | 1 x 1 | 8 | Established | Exact sampling and readback |
| R32G32B32A32_UINT | 1 x 1 | 16 | Established | Exact sampling and readback |
| R32G32B32A32_SINT | 1 x 1 | 16 | Established | Exact sampling and readback |
| R32G32B32A32_SFLOAT | 1 x 1 | 16 | Established | Exact sampling and readback |
| BC1_RGB_UNORM_BLOCK | 4 x 4 | 8 | Established | Exact sampling and readback |
| BC1_RGB_SRGB_BLOCK | 4 x 4 | 8 | Established | Exact sampling and readback |
| BC1_RGBA_UNORM_BLOCK | 4 x 4 | 8 | Established | Exact sampling and readback |
| BC1_RGBA_SRGB_BLOCK | 4 x 4 | 8 | Established | Exact sampling and readback |
| BC2_UNORM_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC2_SRGB_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC3_UNORM_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC3_SRGB_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC4_UNORM_BLOCK | 4 x 4 | 8 | Established | Exact sampling and readback |
| BC4_SNORM_BLOCK | 4 x 4 | 8 | Established | Exact sampling and readback |
| BC5_UNORM_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC5_SNORM_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC6H_UFLOAT_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC6H_SFLOAT_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC7_UNORM_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| BC7_SRGB_BLOCK | 4 x 4 | 16 | Established | Exact sampling and readback |
| ASTC_4X4_UNORM_BLOCK | 4 x 4 | 16 | Established | Unavailable |
| ASTC_4X4_SRGB_BLOCK | 4 x 4 | 16 | Established | Unavailable |
| ASTC_5X4_UNORM_BLOCK | 5 x 4 | 16 | Established | Unavailable |
| ASTC_5X4_SRGB_BLOCK | 5 x 4 | 16 | Established | Unavailable |
| ASTC_5X5_UNORM_BLOCK | 5 x 5 | 16 | Established | Unavailable |
| ASTC_5X5_SRGB_BLOCK | 5 x 5 | 16 | Established | Unavailable |
| ASTC_6X5_UNORM_BLOCK | 6 x 5 | 16 | Established | Unavailable |
| ASTC_6X5_SRGB_BLOCK | 6 x 5 | 16 | Established | Unavailable |
| ASTC_6X6_UNORM_BLOCK | 6 x 6 | 16 | Established | Unavailable |
| ASTC_6X6_SRGB_BLOCK | 6 x 6 | 16 | Established | Unavailable |
| ASTC_8X5_UNORM_BLOCK | 8 x 5 | 16 | Established | Unavailable |
| ASTC_8X5_SRGB_BLOCK | 8 x 5 | 16 | Established | Unavailable |
| ASTC_8X6_UNORM_BLOCK | 8 x 6 | 16 | Established | Unavailable |
| ASTC_8X6_SRGB_BLOCK | 8 x 6 | 16 | Established | Unavailable |
| ASTC_8X8_UNORM_BLOCK | 8 x 8 | 16 | Established | Unavailable |
| ASTC_8X8_SRGB_BLOCK | 8 x 8 | 16 | Established | Unavailable |
| ASTC_10X5_UNORM_BLOCK | 10 x 5 | 16 | Established | Unavailable |
| ASTC_10X5_SRGB_BLOCK | 10 x 5 | 16 | Established | Unavailable |
| ASTC_10X6_UNORM_BLOCK | 10 x 6 | 16 | Established | Unavailable |
| ASTC_10X6_SRGB_BLOCK | 10 x 6 | 16 | Established | Unavailable |
| ASTC_10X8_UNORM_BLOCK | 10 x 8 | 16 | Established | Unavailable |
| ASTC_10X8_SRGB_BLOCK | 10 x 8 | 16 | Established | Unavailable |
| ASTC_10X10_UNORM_BLOCK | 10 x 10 | 16 | Established | Unavailable |
| ASTC_10X10_SRGB_BLOCK | 10 x 10 | 16 | Established | Unavailable |
| ASTC_12X10_UNORM_BLOCK | 12 x 10 | 16 | Established | Unavailable |
| ASTC_12X10_SRGB_BLOCK | 12 x 10 | 16 | Established | Unavailable |
| ASTC_12X12_UNORM_BLOCK | 12 x 12 | 16 | Established | Unavailable |
| ASTC_12X12_SRGB_BLOCK | 12 x 12 | 16 | Established | Unavailable |

Packed depth/stencil formats require separate copy planes and an explicit
guest padding rule. D16_UNORM_S8_UINT, D24_UNORM_S8_UINT and
D32_SFLOAT_S8_UINT have established Vulkan meanings, but no established guest
storage rule. They remain unsupported by the general mapped transfer path.
The earlier dedicated depth/stencil attachment path retains its explicit host
choice. Three-component, scaled and additional extension formats remain
outside this transfer catalogue.

## Target and storage rules

All guest associations in this table are hypotheses. Host image and view
relationships are established by the Vulkan image view requirements.

| Explicit host shape | Image type | Sampled view | Dimension rule |
| --- | --- | --- | --- |
| 1D | 1D | 1D | Height, depth and layers equal one |
| 1D array | 1D | 1D array | Height and depth equal one; count selects layers |
| 2D | 2D | 2D | Depth and layers equal one |
| 2D array | 2D | 2D array | Depth equals one; count selects layers |
| 3D | 3D | 3D | Count selects depth; layers equal one |
| Cube | 2D with cube compatibility | Cube | Square extents, six layers |
| Cube array | 2D with cube compatibility | Cube array | Square extents, layer count divisible by six |

Multisample and buffer targets have no rule in this path. Cube array support
and format support depend on enabled device features.

Storage rules select linear or tiled bytes and match the complete flags
integer. Tiled rules include explicit height and depth exponents from zero
through five. Nonzero stride accepts only a tight, single-level linear image.
The [tiled transfer note](0031-tiled-texture-transfers.md) defines the packing.

ImageEnums compiles the product of host-supplied format, target and storage
rules into exact tuple rules. ImageRule retains each field's confidence and
rationale. A host with correlated restrictions can supply individual tuple
rules instead. The library supplies no default guest numbering.
Duplicate tokens, empty rationales, invalid selectors, invalid geometry and
unknown tuples fail.

## Component and aspect selection

| Explicit host component | Host result | Guest association |
| --- | --- | --- |
| Identity | Corresponding source channel | Hypothesis, host token required |
| Zero | Numeric zero | Hypothesis, host token required |
| One | Integer one or float one | Hypothesis, host token required |
| R | Source red component | Hypothesis, host token required |
| G | Source green component | Hypothesis, host token required |
| B | Source blue component | Hypothesis, host token required |
| A | Source alpha component | Hypothesis, host token required |

A nonempty swizzle contract resolves every recorded component independently.
Unknown selectors fail before allocation. Sampled views cache the complete
component mapping; attachment views keep identity selection. Image recycling
retains valid views for the same image. Removing a resource invalidates cached
transfer recordings before destruction.

Depth/stencil mode rules require exactly one supported aspect. Unknown modes
or the wrong aspect fail. An empty mode table retains the earlier explicit
zero-mode host choice. An empty component table retains the earlier host
identity check in recorded drawing.

## Sampler rules

All numeric guest associations remain hypotheses. EnumRule carries an
Established or Hypothesis tag and a required rationale. The tag describes
the host's evidence claim; the library cannot verify that claim from a string.

| State | Explicit host choices | Creation rule |
| --- | --- | --- |
| Minification | Nearest or linear texel filter, independently nearest or linear mip filter | Resolve the combined pair from one guest token |
| Magnification | Nearest, linear | Resolve independently of minification |
| Wrap per axis | Repeat, mirrored repeat, clamp to edge, clamp to border | Resolve all three tokens |
| Compare mode | Disabled, enabled | Enabled requires a mapped comparison function |
| Compare function | Never, less, equal, less or equal, greater, not equal, greater or equal, always | Resolve only when comparison is enabled |
| Anisotropy | Explicit opt-in | Finite value at least one, enabled device feature, device maximum |
| Border | Transparent black, opaque black, opaque white | Match exact fixed colors; host selects float or integer kind |
| LOD | Explicit float order and finite bias/bounds | Nonnegative minimum, ordered bounds, device bias limit |

Other border colors and unsupported address modes fail. The existing
TextureContract filter/wrap lists remain explicit legacy host choices.
SamplerEnums supplies tagged rules with distinct minification and
magnification domains. Sampler cache keys include comparison, anisotropy,
border color, all address modes, filter pair and LOD bits.

The bounded recorded draw path reflects float and depth sampled images.
Integer images have native transfer and direct host sampling coverage, but
recorded integer draw bindings remain unsupported. Registered subresource
views, multisampling and arbitrary border colors remain open.

## Verification

The catalogue GPU test queries every host format, uploads original encoded
payloads, checks active readback bytes, then samples each supported format
with all seven component selectors. Float samples compare exact bit patterns.
Integer samples compare exact values. Original nonzero four-channel texels
separately prove that each selector chooses the intended source channel.

Sampler GPU probes compare exact values for nearest and linear filtering,
repeat, mirrored repeat, edge and fixed border modes, and both outcomes of
depth comparison. The tiled mip test now samples 1D and 1D array images as
well as the earlier image shapes. CPU checks exercise their packing and
reject invalid dimensions. Tests classify their synthetic guest rules as
hypotheses. None of these fixtures is observed program data.

Test shader output and isolated translator packages use the configured
temporary directory. Nested translator builds use CARGO_TARGET_DIR.
Generated files and driver caches stay outside source directories.

Run formatting, strict all-target/all-feature clippy, the minimal-feature
suite and the full all-feature suite with ignored GPU tests enabled.
Translator checks require the externally configured NOVENA_SHADOWBOX_PATH.
No established guest format sampling claim is made, because the permitted
records establish no numeric guest format interpretation.

## Recorded check result

Formatting, strict all-target/all-feature clippy and the minimal-feature suite
pass. All eight image GPU tests pass. The catalogue samples and reads back
73 supported host formats exactly and reports 29 unavailable host formats.

The full all-feature run with ignored tests enabled exits 101. Seven checks
fail across the translated drawing, global memory and compute pipeline suites.
The original triangle check fails on both this change and base commit c54f5ac.
Disassembly of its translated synthetic fragment program shows a constant-zero
color output instead of the fixture's requested color. This prevents a claim
that the full suite passes. The external translator requires a separate repair.

Independent temporary shader files also remove interference between parallel
source-based drawing checks. All seven source-based drawing checks pass after
that correction. No failing check was removed or disabled.
