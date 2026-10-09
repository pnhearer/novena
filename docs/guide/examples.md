# Runnable examples

Both examples use original data and the existing function table. Their small
integer format and state tokens are explicit host choices. They do not claim
to establish unknown guest enum values.

Set `CARGO_TARGET_DIR` and `TMPDIR` to build and scratch directories outside
the source tree. Run the commands from the repository root.

## Cleared window

[clear_window.c](../../examples/clear_window.c) creates a texture and window
object, records a blue clear, submits its command handle, and receives RGBA
pixels through the host presentation callback. X11 displays those copied pixels.
This example uses the default CPU build and needs a C compiler and X11
development headers. It needs no Vulkan loader or translator.

```sh
cargo build --workspace
cc -std=c11 -Wall -Wextra -Werror -Iinclude examples/clear_window.c \
  -L"$CARGO_TARGET_DIR/debug" -lnovena -lX11 \
  -Wl,-rpath,"$CARGO_TARGET_DIR/debug" -o "$CARGO_TARGET_DIR/clear-window"
"$CARGO_TARGET_DIR/clear-window"
```

Press a key or close the window to exit. The image has a fixed size.
Without a display, run `"$CARGO_TARGET_DIR/clear-window" --check`.
It executes the same library calls and checks every pixel before returning.
Build without Vulkan to exercise the CPU executor.

For native surface and swapchain integration, the existing
[presentation example](../../examples/present.c) uses `HostVulkan`.
The [architecture guide](architecture.md) explains the two presentation paths.

## Textured triangle

[textured_triangle.rs](../../crates/novena/examples/textured_triangle.rs)
uses Vulkan to draw a black and white checkerboard triangle on blue.
It needs a compatible Vulkan device, `glslangValidator`, and `spirv-val`.
It needs no display or external translator.

```sh
cargo run --features vulkan --example textured_triangle -- "$CARGO_TARGET_DIR/triangle.ppm"
```

Open the PPM image with an image viewer. The example also checks clear,
white, and black pixels, plus coverage and texture colors away from triangle
edges. Failure returns a nonzero process status.

The example creates a mapped pool, uploads three original vertices and four
RGBA texels, registers a texture and sampler, and records a draw. Its local
`ShaderTranslator` hook maps synthetic markers to SPIR-V compiled from original
GLSL. It does not translate hardware instructions. The markers use only the
existing shader envelope so registration follows the normal bounded read path.

The host installs explicit topology, vertex-format, texture-slot, filter, and
wrap contracts. The first submission may skip a draw while its pipeline
compiles. The example resubmits until pixels arrive or a 30-second timeout
expires. It checks diagnostics and writes the completed arena bytes.
Temporary shader files are removed after compilation.

Evidence: [CPU clears](../provenance/0012-cpu-clears.md),
[presentation](../provenance/0026-presentation.md),
[shader envelope](../provenance/0021-program-layout.md),
[textured drawing](../provenance/0030-textured-blended-drawing.md), and
[example provenance](../provenance/0033-newcomer-guide.md).
