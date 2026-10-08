# Signatures 0009: draw state

This note restates only facts already present in signatures 0001 through 0004.
It does not assign meanings to enumerations or describe unobserved layouts.

| Command | Established shape and observation | Establishes | Gap |
| --- | --- | --- | --- |
| DrawArrays | commands, primitive, first, count; observed primitive 0, first 0, count 4 | A non-indexed draw call has four arguments | Primitive values and execution semantics |
| DrawArraysInstanced | commands, primitive, first, count, baseInstance, instances; primitive 0, first 0, count 4, baseInstance 0, final values 0x20 or 0x40; order guessed | An instanced non-indexed draw has this shape | Final argument order and instance semantics |
| DrawElementsBaseVertex | commands, primitive, indexType, count, indices, baseVertex; primitive 4, index type 1 or 2, count 3 or 6, wide indices, base vertex 0 | An indexed draw has this shape | Index meanings, memory layout, primitive semantics |
| BindVertexBuffer | commands, index, buffer, size; index 0, wide buffer, size | A vertex stream can be bound by index, address, and size | Address and element layout |
| BindVertexStreamState | commands, count, states; count 1 and one of four state addresses | Vertex stream state is a counted pointer array | State layout and field meanings |
| BindVertexAttribState | commands, count, states; count 1, 2, or 5 and one of four state addresses | Vertex attributes are a counted pointer array | State layout and field meanings |
| SetViewport | commands, x, y, width, height; x and y always zero; sizes include 0x500 by 0x2d0 and 0x780 by 0x438 | A viewport has integer origin and size | Coordinate and depth convention |
| SetScissor | Same shape as SetViewport; x and y always zero and final values are sizes | A scissor has integer origin and size | Bounds and coordinate convention |
| BindBlendState, BindChannelMaskState, BindColorState | commands, state; addresses observed | Blend and color state are bound objects | Layout, enable, factors, operations, masks |
| BindDepthStencilState | commands, state; two addresses observed | Depth and stencil state are bound as an object | State layout and meanings |
| SetRenderTargets | commands, count, colors, colorViews, depth, depthView; count 0, 1, 2, or 6; pointers readable or zero; depth always zero in this run | Render targets use a count, color arrays, and optional depth arguments | Array layout, views, formats, attachment semantics |
| SetDepthRange | commands, near, far; near always 0 and far 0 or 1 | A depth range has two floating point arguments | Exact ABI and viewport mapping |

The three draw shapes and the already decoded viewport, scissor, depth
range, and render-target fields are decoded in the recording list.
[Signatures 0010](0010-remaining-command-state.md) adds explicit records for
firm state fields. Unresolved calls still retain raw registers.
