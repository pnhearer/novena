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

## Run against one program (2026-10-06)

With these handlers answering, one program presented 3,561 frames in 90 seconds without a fault. Every saved frame was 1920 by 1080 and black: the program clears the presented texture to opaque black and draws everything else, and draws are not executed yet.

Three changes came out of that run, all novena's own:

- Only textures a window presents get a CPU image. Filling CPU copies of every render target the program clears was too much work and memory.
- A recording is moved, not copied, when it ends, is taken (removed) when submitted, and at most the last 64 unsubmitted recordings are kept. A recording handle carries its command buffer's address in its low 48 bits, so the lookup is direct.
- Submit and present report zero in the result register, as before they had handlers.
