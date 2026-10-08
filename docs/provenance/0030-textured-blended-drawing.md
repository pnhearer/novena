# 0030: textured and blended drawing

- Date: 2026-10-08
- Author: Khargoosh
- Covers: descriptor pools, sampled images, samplers, blend state, channel masks,
  multiple color targets and graphics cache identity

## Evidence boundary

[Signatures 0003](../signatures/0003-objects.md#pools-and-handles) record texture
and sampler registration ids, object references and separate handle results.
Texture registration has an optional view reference. The separate handles
returned by this library use the registration id, an existing implementation
choice. No observed handle bit layout is inferred.

[Signatures 0010](../signatures/0010-remaining-command-state.md) establish the
stage, index and opaque reference retained by `BindSeparateTexture`.
[Signatures 0011](../signatures/0011-command-evidence.md)
leave the sampler reference's handle-versus-pointer meaning open. Execution
resolves only ids returned by this library in an explicitly selected pool.
It never dereferences the sampler reference. Pool selection follows the
recorded two-object shapes in signatures 0003.

`BindTexture` appears in the census as requested with zero calls. Its
stage/index/reference recording follows the separate-binding family as an
explicit hypothesis. There is no observed combined-handle representation.
A host must supply a table from opaque combined references to separate texture
and sampler ids. An absent entry returns `Unimplemented`. No combined handle
bits, descriptor memory or pointed-to storage are decoded.

[Signatures 0001](../signatures/0001-setup.md) record min/mag filter integers,
three wrap integers, anisotropy in d0, compare fields and initialization.
[Signatures 0004](../signatures/0004-pointers.md) establish four border-color
floats. Filter enums, wrap enums, disabled comparison and the conventional
min/mag and axis orders remain host hypotheses. The bounded path requires
anisotropy one, zero bias, zero LOD bounds and a transparent black border.
Other sampler fields return `Unimplemented`.

[Signatures 0003](../signatures/0003-objects.md#state-objects) record blend
selection, four factors, two equations, per-target enable and four channel
flags. Their enum meanings and argument order are not established. A separate
host contract supplies factor and operation tables and explicit argument
permutations. The final `BlendTarget` selects the attachment changed by a
bound blend snapshot, a host hypothesis. Later bindings for another target
preserve the earlier target's state. No state object offsets are read.

## Public host and translator interface

The selected translator emits descriptor decorations in its SPIR-V words.
Its separate image/sampler pairs use set zero for distinct bindless handle
sources and set one for bound table entries. The public hook retains these
words. Reflection reports each descriptor's original set, binding and type;
the host contract maps recorded slots to those reported pairs. This change
copies no translator implementation or guest shader bytes.

The accepted image shape is a singleton, non-array, non-multisampled,
non-comparison float 2D sampled image. Separate singleton samplers are accepted.
Storage images, combined sampled-image declarations, descriptor arrays, other
image shapes and unrelated resource kinds are rejected. Existing constant-bank
shape validation continues to apply alongside textures.

The original machine-code texture proof uses the public
[envytools instruction table](https://github.com/envytools/envytools/blob/master/envydis/gm107.c).
TEX.B.LZ uses opcode 0xdeb8, destination bits 0 through 7, coordinate register
bits 8 through 15, handle register bits 20 through 27, 2D selection bit 29,
component mask bits 31 through 34 and level-zero bit 37. Original immediate
moves provide controlled coordinates and a constant handle source. Header,
MOV, position output, bundles and EXIT reuse the public facts in provenance
0027. The test supplies only original shaders and texels.

Host behavior follows public Vulkan documentation:

- [Descriptor sets](https://docs.vulkan.org/spec/latest/chapters/descriptorsets.html)
  define separate image and sampler descriptors, updates and layouts.
- [Samplers](https://docs.vulkan.org/spec/latest/chapters/samplers.html)
  define normalized coordinates, nearest and linear filters, addressing and borders.
- [Framebuffer blending](https://docs.vulkan.org/spec/latest/chapters/framebuffer.html)
  defines per-attachment factors, equations and write masks.

Only the permitted repository evidence, the selected translator interface,
public instruction facts and public Vulkan documentation informed this work.

## Implementation choices and limits

Uniform banks occupy vertex set zero and fragment set one. Textures occupy
vertex set two and fragment set three. Original texture sets zero and one
become binding ranges zero through 255 and 256 through 511. Descriptor counts,
resource totals and device layout support are checked. The shader rewrite
changes decorations, preserving the sampling instructions and bank loads.
The descriptor pool survives synchronous draw completion.

Pool-backed tightly packed RGBA8 base levels use Vulkan sampled images. Each
texture object owns one cached image view. Initialization and finalization
discard the old image and view. Every sampled draw reloads canonical arena
bytes, then transitions to shader-read layout with a shader-read dependency.
Samplers are cached by pool and registration id. The interpreted filter and
wrap key is compared on every draw, so re-registration or changed host choices
replace stale samplers. A combined binding sets both references; a separate
setter replaces only its corresponding reference.

One through eight color targets are accepted subject to device limits. All
have matching extents, one level, one layer and the existing explicit format
and swizzle contract. Shader float4 output locations match the target indices.
Overlapping arena attachment ranges and sampled attachment feedback are
unsupported, including aliases involving depth storage. Registered views,
additional levels, arrays, format conversion and comparison sampling remain
unsupported. State inheritance across recordings remains open.

Each active target requires an explicit blend enable. Enabled targets require
bound factor and equation settings. The host can select basic add, subtract,
reverse subtract, min and max operations and ordinary nonconstant factors.
Constant-color factors, dual-source blending, logic operations and multisampling
remain unsupported. An absent channel-mask binding preserves the original
all-channel host experiment. Bound masks require the explicit channel-order
contract. Independent blend state requires the corresponding device feature.

The graphics key includes target count and every interpreted target's enable,
four factors, two equations and write mask alongside the existing draw state,
shader words and uniform storage choice. Interface version three gives these
layouts a new persistent cache namespace. Texture and sampler references and
uniform contents do not change pipeline identity.

## Pixel proofs

`translated_texture_pixels` translates an original texture instruction and
reads every pixel back. It checks texture re-registration, reinitialization
with changed dimensions and partial replacement of combined references.

`textured_checkerboard_and_blend_pixels` checks all 4096 pixels for nearest
and linear filtering with repeat, mirrored repeat, clamp to edge and clamp to
border. It runs every case through separate and combined references. The
CPU reference addresses individual texels and computes bilinear weights.
Linear results allow two byte values of rounding tolerance. The same shader
blends half-alpha checker texels against blue and verifies independent color
and alpha factors plus preserved channels. Missing supported view semantics
and comparison sampling return `Unimplemented`. Changing only resources
reuses one pipeline; enabling blending and changing a mask create distinct keys.

`multiple_target_blend_pixels` binds blend snapshots in reverse target order.
The first target uses source alpha against blue. The second uses replacement
color with preserved destination alpha and red masked off. Both attachments'
canonical bytes match. Changing only the second target's mask changes its
pixels and creates a second pipeline key.

`textured_uniform_banks_and_persistence` samples beside fragment bank two,
loads a nonzero shader byte offset and changes only the tint data. Both uniform
and forced read-only storage descriptors produce matching pixels. Repeated
data changes reuse a pipeline. The second instance reopens the saved driver
cache with the same translation identity.

Run the bounded proofs with a configured translator:

```sh
python3 scripts/check-drawing.py --textures
```

The check refuses optional skips, nonzero exits and validation errors. Full
verification also runs formatting, default and full-feature clippy with warnings
denied, default tests and every full-feature test including ignored GPU checks.
These are host experiment results. Guest enum assignments, combined handles,
slot mapping and blend argument order remain hypotheses.
