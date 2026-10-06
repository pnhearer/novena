# 0012: CPU clears

- Date: 2026-10-06
- Author: Khargoosh

This change adds a novena-designed recording list. Unknown command arguments
are retained as the function id and eight integer registers. Nothing new was
observed from any program for this design.

Textures lazily receive a shared base-level RGBA8 CPU image. This is novena's
own choice while Vulkan is not present. The channel mask uses bits 0 through 3
for red, green, blue and alpha. This is novena's own assumption based on the
observed mask argument and four-float color pointer. Viewport and scissor are
recorded but ignored by the first CPU clear executor. Draw commands are
recorded and skipped. The optional version-3 `present` callback is novena's
own host boundary for passing an image to the host.
