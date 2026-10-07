# 0015: draw scaffolding

- Date: 2026-10-07
- Author: Khargoosh

The draw and draw-state facts in docs/signatures/0009-draw-state.md come from
signatures 0001 through 0004. No new observation was made. The recording list
decodes DrawArrays, DrawArraysInstanced, and DrawElementsBaseVertex using only
established register positions. Viewport, scissor, depth range, and render
targets keep their prior decoded fields. Blend, depth, vertex, attribute, and
other state calls remain raw.

Implementation assumptions, not observed target behavior:

- CPU submission counts recorded draw commands and skips execution.
- Vulkan execution is skipped when no device or shader translator is available.
- A Vulkan draw uses one fixed pipeline layout with no descriptors or push
  constants, triangle-list topology, one RGBA8 color attachment, and the
  recorded viewport and scissor.
- Shader bytes are passed to the translator only when the hook can provide
  them. Existing shader observations do not establish code addresses or sizes,
  so draws are skipped when bytes cannot be obtained.
- Render-target images use the existing RGBA8 choice and one-shot submission.
  These are novena implementation choices, not target API facts.

This is scaffolding only. Vertex contents, attribute formats, blend behavior,
depth behavior, target formats, and shader record layouts remain open.
