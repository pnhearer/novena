# Drawing support and open questions

Novena executes controlled non-indexed lists, strips, and fans through its
Vulkan queue path. It decodes multiple attributes from shared or separate
streams. Guest enum mappings remain open. The experiment uses an explicit
host contract and original synthetic shaders. This document distinguishes
that implementation from facts in `docs/signatures` and `docs/provenance`.

## First draw

With `vulkan`, a Rust host enables the experiment through
`Instance::set_first_draw_contract`. `FirstDrawContract` supplies host-selected
raw token tables for topology and attribute formats, plus no culling, RGBA8,
a 2D target, and identity swizzle. It also supplies optional object spacing
for counted attribute and stream bindings. These choices do not establish
guest enums or layouts. Unknown or duplicate tokens are unsupported.
No contract means a submitted draw returns `Unimplemented`.

The supported recording has these limits:

| Input | Executed behavior |
| --- | --- |
| `DrawArrays` | One instance, recorded first vertex and count, host-selected triangle list, strip, or fan; restart disabled |
| Vertex buffer | Each active stream has a pool-resolved address and bounded size in the flat arena |
| Vertex stream state | Count at most 16, recorded byte stride, divisor explicitly zero; zero stride repeats one element |
| Vertex attribute state | Count at most 16, explicit stream and byte offset, host-selected format; each array index maps to the same shader location |
| Program | Exactly one translated vertex `main` and one fragment `main`, paired by SPIR-V execution model |
| Shader interface | Consecutive 32-bit float scalar or vector inputs starting at location zero, matching float varyings, one float4 color output at location zero |
| Color target | One pool-backed 2D base level, tightly packed RGBA8, no views, zero flags and depth-stencil mode, host-selected identity swizzle |
| Viewport and scissor | Explicit zero origin, positive sizes bounded by the target, Vulkan pixel coordinates |
| Color state | Target zero, blend enable explicitly zero, all channels written |
| Depth and stencil state | Test, write, and stencil enable explicitly zero, no depth attachment |
| Polygon state | Explicit host-selected no-cull token, fill mode and one sample |
| Depth range | Vulkan interval zero through one, optional recorded interval must match |

Bindings capture side-table settings, so later setters do not change an
earlier binding. Count-one stream-state bindings identify the state object
itself, as signatures 0004 describes. Direct attribute-state identity remains
an experiment choice. Counts above one require explicit host object spacing.
Guest array layouts remain open. No object stride is inferred.
Each recording starts with empty draw state. State inheritance across recordings
is open. Program translation is retained by program and read at submission.
Program replacement, finalization, and late-read generation handling remain open.
The existing shader reader's 0x40 record stride for count two remains an
implementation assumption. The proof does not establish that stride for guests.

Unsupported fields, missing explicit state, unknown recorded commands,
resources, or unsupported shader interfaces return `Unimplemented`.
Bad addresses, short vertex ranges, invalid sizes, and missing objects return
`BadArgument`. Vulkan draw submission failures return `InternalError`.
Pipeline creation failures return `Unimplemented`. Feature negotiation remains open. Indexed and instanced draws are open.
The experiment rejects bound blend, channel-mask, and multisample objects whose
fields it cannot interpret. Only the listed state fields are supported.
The Rust experiment has no C configuration interface yet.

Graphics compilation runs on bounded background workers. On first use, the
executor queues the retained stage words and polls once. A queued, compiling
or queue-full result skips only that draw and allows other commands to continue.
Skipped draws are not replayed. Failures remain visible until explicit retry.
This is a host scheduling choice, not observed guest behavior.

A host enables private disk persistence outside submission with
`Instance::set_graphics_pipeline_cache`, supplying a directory, translation
identity and worker limits. Workers save checked driver-cache snapshots.
Reopening loads compatible data or falls back to an empty cache. The graphics
and compute caches share the persistence and worker implementation, with
distinct interface namespaces. Graphics uses retained translations and does
not translate again. Cache diagnostics and persistence statistics are available
through the Rust instance. See [shader pipelines](pipelines.md).

The backend creates a color-attachment image, loads its canonical arena bytes,
uses a load/store render pass, and stores the rendered bytes back to the arena.
The vertex binding references the arena buffer directly. Attachment transfers,
vertex reads, and color writes have Vulkan memory dependencies. A fence keeps
the pipeline, image view, framebuffer, and pool alive through completion.
Presentation uses the existing offscreen blit and readback path.
These are Novena implementation choices, not observations of target behavior.

The optional proof runs with a selected translator checkout:

```sh
cargo test --features shadowbox --test shadowbox_drawing -- --ignored --nocapture
```

`NOVENA_SHADOWBOX_PATH` selects the separate translator crate. With that variable
set, translation, SPIR-V validation, Vulkan availability, and pixel mismatches
fail the test. Without it, the optional wrapper reports a skip.
The proof records a padded vertex buffer with nonzero pool offset, first vertex,
and attribute offset. It checks covered and uncovered pixels in presentation
readback and canonical pool storage. Repeated draws reuse the pipeline.
Changed fragment content creates a new pipeline and changes the pixels.
The proof also checks driver-cache reopening, damaged-cache recovery and reuse
from a fresh process.
[Provenance 0027](../provenance/0027-drawing.md) records the sources and checks.

Steps 2 and 3 have a separate repeatable check:

```sh
python3 scripts/check-drawing.py
```

The check requires a configured translator, Vulkan, `spirv-val`, and
`glslangValidator`. It refuses skips. The topology proof uses translated
synthetic instructions for both observed primitive tokens and a synthetic fan
token. An additional translated input changes position W and pixel coverage.
The format proof uses original source shaders, two inputs, and a matching
varying. It checks twelve formats, shared and separate streams, nonzero
offsets, varied byte strides, zero stride, snapshots, and exact fetch bounds.
All cases compare canonical target bytes with presentation readback.
[Provenance 0028](../provenance/0028-vertex-decoding.md) distinguishes these host
proofs from guest hypotheses and lists the real-program confirmation tests.

All enum conversion beyond the host contract, integer shader inputs,
packed attribute formats, nonzero viewport origins, coordinate conversion,
specialization overrides, textures, samplers, uniforms, index data, instancing,
depth, stencil, blend, masks, culling, polygon variation, multisampling, views,
array layers, additional levels, multiple targets and asynchronous draw
submission remain open. The sections below list the observations
needed to close these gaps.

## Vertex buffers and attribute formats

The signatures establish one `BindVertexBuffer` shape with stream index 0, a GPU address, and a size. They establish counted arrays for `BindVertexStreamState` and `BindVertexAttribState`. `VertexStreamStateSetStride` takes a stride, `VertexStreamStateSetDivisor` takes 0 or 1, `VertexAttribStateSetFormat` takes a format-like value and an offset, and `VertexAttribStateSetStreamIndex` takes a stream index from 0 through 3. The first word behind a vertex stream state matches a stride value. See [command buffer state binding](../signatures/0002-command-buffer.md#state-binding), [vertex state objects](../signatures/0003-objects.md#state-objects), and [pointer arrays](../signatures/0004-pointers.md#arrays-of-objects).

Registered pools resolve vertex addresses into the flat arena. The first draw
proof checks that mapping with a nonzero offset. Step 3 adds a second attribute,
format conversion, and separate streams through the host contract.
[Provenance 0028](../provenance/0028-vertex-decoding.md#format-mappings)
lists all twelve `VkFormat` mappings and their confirmation tests.
Guest format tokens, attribute locations, object spacing, the complete
stream-state layout, and divisor semantics remain open.

The smallest useful observation is one draw with one vertex buffer and one attribute. Vary the attribute format, offset, stream index, and stride one at a time. Dump the state object at offset `+0x00` and later words with labels. Vary the vertex buffer pool offset and compare the draw's address with the pool base. Read the vertex bytes at the resolved offset and compare a known color or position change in the result.

## Index buffers

`DrawElementsBaseVertex` establishes a primitive value, an index-type value of 1 or 2, a count, a GPU-shaped index address, and base vertex 0. See [drawing and clearing](../signatures/0002-command-buffer.md#drawing-and-clearing) and [draw state](../signatures/0009-draw-state.md).

Missing knowledge includes the index element widths represented by 1 and 2, the address mapping and byte layout, the meaning of `baseVertex`, and whether the count is an index count. No index buffer binding call is established. The draw carries the index address directly.

The smallest useful observation is two indexed draws over the same three indices, with the index type changed once and `baseVertex` changed once. Vary the index address and count separately. Compare the resolved bytes, the vertex selected, and the resulting primitive.

## Primitive topology

`DrawArrays` and `DrawElementsBaseVertex` both carry a `primitive` value. The observed values are 5 for `DrawArrays` and 4 for indexed draws. The signatures do not map either value to a topology. See [draw state](../signatures/0009-draw-state.md).

Missing knowledge is the value-to-topology map, including the number of vertices consumed and the restart behavior, if any.

Step 2 implements host-selected lists, strips, and fans. The experimental
choices 4 to triangle list and 5 to triangle strip remain hypotheses. Counts
alone cannot distinguish a strip from a fan. The fan test uses a synthetic
token, with no proposed guest value. The maps and an asymmetric real-program
confirmation test are in [provenance 0028](../provenance/0028-vertex-decoding.md#topology-mappings).

The smallest useful observation is the same three or four vertices drawn with one primitive value at a time. Change only the primitive value and inspect the number and arrangement of generated vertices or fragments.

## Viewport and scissor

`SetViewport` and `SetScissor` carry integer origin and size. The observed origins are zero. Viewport sizes include 1280 by 720 and 1920 by 1080. `SetDepthRange` carries near and far values, with near 0 and far 0 or 1. See [command buffer state binding](../signatures/0002-command-buffer.md#state-binding), [set-up calls](../signatures/0001-setup.md#command-buffer), and [draw state](../signatures/0009-draw-state.md).

Missing knowledge includes the coordinate origin, pixel-center rule, viewport depth convention, scissor inclusion bounds, and whether the integer values are pixels without further scaling. The current Vulkan image scale and one-shot submission are novena choices, not target facts. See [Vulkan backend](../provenance/0013-vulkan-backend.md).

The smallest useful observation is one point or rectangle moved across each edge. Vary one origin, width, height, near value, or far value at a time. Compare the first and last affected pixels and the depth value written.

## Blend

Blend state is set through `BindBlendState`, `BindChannelMaskState`, and `BindColorState`. The setters establish per-target selection, four blend-function integers, two blend-equation integers, channel mask flags, and a blend enable flag. See [state objects](../signatures/0003-objects.md#state-objects) and [command buffer state binding](../signatures/0002-command-buffer.md#state-binding).

Missing knowledge includes the layout of each state object, the source and destination factor order, the color and alpha factor order, equation meanings, target numbering, and channel-mask order. No observed call establishes the resulting blend operation.

The smallest useful observation is two overlapping draws with known source and destination colors. Toggle `ColorStateSetBlendEnable`, then vary one of the four blend-function values and one equation value. Toggle one channel-mask value at a time and identify the affected output channel.

## Depth and stencil

`DepthStencilStateSetDepthTestEnable`, `DepthStencilStateSetDepthWriteEnable`, and `DepthStencilStateSetStencilTestEnable` each take a 0 or 1. The depth function takes values from 2, 3, 4, 5, 7, and 8. Stencil function and operation calls carry face, function, reference, mask, and three operation-like values. Command calls also set stencil masks and reference values. See [state objects](../signatures/0003-objects.md#state-objects) and [command buffer state binding](../signatures/0002-command-buffer.md#state-binding).

Missing knowledge includes the state-object layout, depth comparison mapping, depth write behavior, stencil face values, comparison mapping, operation order, reference and mask order, depth attachment format, and depth coordinate convention. `ClearDepthStencil` has an open argument interpretation. The backend's D32 depth image is an implementation choice.

The smallest useful observation is two overlapping draws at known depths, followed by one stencil comparison and one stencil operation. Toggle each enable and write flag. Vary one depth-function value, one stencil-function value, one reference, one mask, and one operation at a time. Inspect the depth and stencil attachment after each draw.

## Rasterizer

`BindPolygonState` binds a polygon state object. Its setters establish cull-face values 0, 1, and 2, polygon-mode values 0, 1, and 2, and an open three-float `SetPolygonOffsetClamp` command. A multisample state object is also bound, but its fields are not established. See [state objects](../signatures/0003-objects.md#state-objects) and [command buffer state binding](../signatures/0002-command-buffer.md#state-binding).

Missing knowledge includes front-face winding, the meaning of cull-face values, polygon modes, polygon offset fields, sample count and sample masks, and the layout of the bound objects.

The smallest useful observation is one front-facing and one back-facing triangle. Reverse the vertex order and vary one polygon value at a time. Then vary each offset float with a sloped triangle and vary the multisample state with an edge that crosses a pixel.

## Render targets and formats

`SetRenderTargets` carries a count, a color-object array, a color-view array or null, an optional depth object, and an optional depth-view pointer. The color array is an array of texture object addresses. Texture builders establish target, format, levels, dimensions, stride, swizzle, depth-stencil mode, and pool storage offset. Texture views establish level and layer ranges. See [command buffer state binding](../signatures/0002-command-buffer.md#state-binding), [texture objects](../signatures/0003-objects.md#textures), and [pointer arrays](../signatures/0004-pointers.md#arrays-of-objects).

Missing knowledge includes the attachment array and view layouts, the target format mapping, row layout, swizzle meaning, level and layer selection, depth format mapping, load and store behavior, and required transitions between uses. Texture format mapping and persistent layout tracking are explicitly open. See [Vulkan backend](../provenance/0013-vulkan-backend.md).

The smallest useful observation is one color target and one depth target whose dimensions and format each vary once. Vary the color count, view pointer, level, layer, and texture format one at a time. Dump the pointed-to view words with offsets. Compare attachment bytes, row stride, and clear or draw results.

## Texture and sampler bindings

`BindSeparateTexture` and `BindSeparateSampler` carry a stage, an index from 0 through 3, and a handle. Texture and sampler pools register objects by IDs. Device calls return texture and sampler handles in a wide result shape. `BindSeparateSampler` is unresolved because its third value looked like an address. `BindImage` carries stage 5, an index from 0 through 2, and an image handle. See [resource binding](../signatures/0002-command-buffer.md#resource-binding), [pools and handles](../signatures/0003-objects.md#pools-and-handles), and [pointer structures](../signatures/0004-pointers.md#structures).

Missing knowledge includes handle width and lookup, stage meanings, texture and sampler index pairing, descriptor shape, sampler field meanings, texture format and swizzle conversion, image access, and layout transitions. The recorded texture copy also leaves its pointer records and format conversion unresolved. See [texture copies](../provenance/0014-texture-copies.md).

The smallest useful observation is one shader that samples one texture through one sampler. Change only the texture ID, sampler ID, stage, or binding index. Use a texture with two distinguishable texels and a sampler setting with an observable edge or filter change. Compare the selected texel and filtered result.

## Uniform buffers and constant banks

`BindUniformBuffer` carries a stage value from 0, 1, or 5, a binding index from 0 through 2, a GPU address, and a size. The signatures do not establish a constant-bank layout or a shader read convention. See [resource binding](../signatures/0002-command-buffer.md#resource-binding).

Missing knowledge includes stage meanings, binding-to-bank mapping, byte alignment, range interpretation, address resolution, and the offset of each constant used by a shader. No record connects a `ProgramSetShaders` shader record to a uniform binding.

The smallest useful observation is one draw whose color depends on one known constant. Keep the program and binding index fixed, vary one word at one byte offset in the buffer, and repeat for each stage and index. The first offset that changes the result establishes the bank mapping for that shader and binding.

## Shader stage pairing per program

`ProgramSetShaders` takes a program, a count that was always 1 in the observations, and a pointer to shader records. Each record has wide values at `+0x00` and `+0x30`, addresses at `+0x08` and `+0x38`, and other observed words. The first wide value is now chosen by novena as the code location. Pool ranges resolve that GPU-shaped value, and a bounded read plus a zero scan can feed a registered `ShaderTranslator`. The translator receives an explicitly unknown stage. Successful SPIR-V and errors are retained per program. See [program shaders](../signatures/0007-program-shaders.md), [retained shader state](../signatures/0008-program-shader-state.md), [GPU address resolution](../provenance/0016-gpu-address-resolution.md), and [optional shader translation](../provenance/0017-shader-translation.md).

Translated SPIR-V now pairs one vertex and one fragment stage within a program. Missing guest knowledge includes the record stride for counts other than 1, which wide value is code, code size, record stage, the meaning of the other fields, the mapping of `BindProgram` stage bits, and the pairing of translated stages with program inputs and outputs. The current first-value and unknown-stage choices are explicit implementation assumptions.

The late-read behavior is a hypothesis, not an established observation: the
maintainer measured shader records that resolve at `ProgramSetShaders` while
their code bytes are still zero, and suspects a later GPU copy recorded in a
command buffer fills the pool. Retries at bind and queue synchronization points
are retained so a later measurement can confirm or reject that hypothesis.

The smallest useful observation is a program with two shader records and a draw that uses both. Vary the `ProgramSetShaders` count and record order. Label each record at `+0x00`, `+0x30`, `+0x08`, and `+0x38`, resolve both wide values through the pool, and compare each translated result with the stage bit passed to `BindProgram`. Vary one stage bit at a time and inspect whether the draw still links and which inputs and outputs connect.

## Ordered first attempts

1. Implemented as the first draw experiment above: a non-indexed draw with one vertex buffer, one attribute, one color target, no texture or uniform buffer, and depth, stencil, blending, and culling disabled. Host tokens select topology and formats. Guest mappings and coordinate semantics remain open.
2. Implemented through an explicit host contract: repeat the first draw with primitive values 4 and 5 and a synthetic fan token over the same controlled vertices. Guest topology hypotheses still require real-program confirmation.
3. Implemented through an explicit host contract: add a second attribute and vary its format, offset, stride, and stream. Twelve `VkFormat` choices pass readback. Guest tokens, array spacing, and location mapping still require real-program confirmation.
4. Add uniform data after its bank mapping is observed.
5. Add indexed drawing after index type, address, and count semantics are observed.
6. Add depth and stencil, then rasterizer variation, after attachment formats and state mappings are observed.
7. Add texture and sampler bindings after handle, stage, descriptor, and format behavior are observed.
8. Add blend and multiple render targets after factor, equation, mask, view, and format behavior are observed.
