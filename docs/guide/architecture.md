# Architecture

A host owns program memory and the native window. It creates an `Instance`,
resolves function names, fills `Registers`, and forwards each call. Memory
callbacks copy bounded ranges. Object addresses are keys in `Objects`; the
library keeps its own records instead of writing a guessed object layout into
program memory. Host callbacks and their user data must stay alive until the
instance is destroyed and must tolerate concurrent calls.

The source entry points are `instance.rs`, `functions.rs`, and `api/mod.rs`.
Evidence: [call boundary](../provenance/0004-call-interface.md) and
[object signatures](../signatures/0003-objects.md).

## Command recording

BeginRecording starts a list on the command-buffer object. State setters and
bindings retain the observed arguments or snapshots of known state objects.
EndRecording returns a handle to a saved list. QueueSubmitCommands reads
handles through the host and executes the saved commands in order.

Recording a command does not establish every argument's meaning or guarantee
that the executor supports it. The drawing path requires explicit Rust host
contracts for unresolved formats, stage bindings, and enum values. These are
host choices, not new observations. Unknown state remains unknown.

See `api/recording.rs`, `api/state_commands.rs`, `api/commands.rs`, and
`api/drawing.rs`. Evidence:
[recording signatures](../signatures/0002-command-buffer.md),
[remaining state](../signatures/0010-remaining-command-state.md), and
[command evidence](../provenance/0028-command-evidence.md).

## Flat arena memory

The Vulkan backend lazily allocates one 1 GiB mapped buffer. Registered pools
occupy 16-byte-aligned ranges of that buffer. Guest addresses start at
`GUEST_BASE`. An `AddressMap` converts them to buffer device addresses with
a wrapping host-minus-guest delta. Translated global-memory modules receive
that delta in an eight-byte push constant at offset zero.

Pool storage is canonical for arena-backed resources. Uploads, readback,
clears, and supported copies share those bytes. Pool release invalidates
dependent textures before freeing the range. Address resolution can also use
observed pool addresses for observation reads; it does not replace an assigned
Vulkan address.

See `global_memory.rs` and `gpu/memory.rs`. Evidence:
[flat memory](../provenance/0022-flat-global-memory.md),
[merge contract](../provenance/0024-gmem-flat-merge.md), and
[address resolution](../provenance/0016-gpu-address-resolution.md).

## Pipelines and caches

The host supplies translated SPIR-V through `ShaderTranslator`. Shader
registration retains an owned result or request. The first draw path supports
bounded vertex streams, indexed geometry, uniform banks, sampled images, and
explicit depth, raster, blend, and write-mask state. It does not implement
general resource binding or every recorded draw command.

Graphics keys include translated stages, vertex input, topology, storage mode,
and pipeline state. Compute keys compare complete program bytes. Bounded
workers compile asynchronously. A pending or queue-full graphics request skips
that draw for the current submission; later commands continue. Failed requests
remain failed until explicitly retried. The host must submit a later draw to
show it after compilation finishes.

Private disk caches retain translated output and Vulkan driver cache data.
They validate identity and integrity before reuse. Disk data is disposable.
It cannot promise that a driver avoids all compilation. Workers retain their
resources and drain when their owner is destroyed.

See `gpu/graphics.rs`, `gpu/pipelines.rs`, `gpu/pipeline_disk.rs`, and
`workers.rs`. Evidence:
[compute](../provenance/0025-compute-pipelines.md),
[persistence](../provenance/0026-persistent-pipelines.md),
[vertex input](../provenance/0028-vertex-decoding.md),
[depth and raster](../provenance/0028-depth-raster.md),
[uniforms](../provenance/0029-uniform-banks.md), and
[textures and blending](../provenance/0030-textured-blended-drawing.md).

## Textures and tiling

The default CPU copy path treats base-level texels as four opaque bytes.
It does not decode a format. The Vulkan image path uses explicit `ImageContract`
rules to interpret flags, target, format, and storage.

`tiling::Layout` checks shape, mip offsets, array strides, and buffer sizes.
It converts packed-linear bytes to and from the implemented block-linear
layout. Padding is not image content. CPU conversion preserves caller padding.
The GPU transfer path shares the mapped arena with images, uses compute
conversion and transfer barriers, and retains bounded scratch and image pools.

The typed path supports 2D, arrays, 3D, cube layers, complete mip chains, and
opaque compressed payloads. Native image support depends on the device.
A host `CopyDecoder` must supply the meaning of opaque region-copy arguments.
Without it, the older whole-image assumption applies. Comparison sampling,
multisampling, and registered subresource views remain outside this path.

See `tiling/mod.rs`, `gpu/image_layout.rs`, `gpu/texture_transfer`, and
`gpu/textures.rs`. Evidence:
[CPU copies](../provenance/0014-texture-copies.md) and
[packing and transfers](../provenance/0031-tiled-texture-transfers.md).

## Presentation

Without native Vulkan integration, the host `present` callback receives RGBA
bytes, dimensions, and row stride. The callback must copy bytes it wants to
retain. CPU presentation uses the retained texture image. Offscreen Vulkan
presentation reads the arena-backed image and can scale the output.

For a native window, `HostVulkan` supplies instance extension names, surface
creation, and drawable size. The library owns the returned surface and swapchain.
A zero drawable size suspends presentation. Resizes and out-of-date results
trigger recreation. Positive present intervals choose FIFO; zero requests
immediate mode where supported, with FIFO fallback. The optional `wait_vblank`
callback runs before presentation.

See `api/queue.rs` and `gpu/present.rs`. Evidence:
[presentation](../provenance/0026-presentation.md) and
[state integration](../provenance/0027-presentation-state-integration.md).
