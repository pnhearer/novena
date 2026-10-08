# Flat global memory

Novena backs all guest GPU pools with one Vulkan buffer. For every live guest
byte address `g`, the host device address is `g + delta`, modulo 2^64.
The delta stays fixed until the backing buffer is destroyed. This replaces
the CPU-address choice in [provenance 0016](../provenance/0016-gpu-address-resolution.md)
when a Vulkan backend is active. CPU-only execution and observation keep their
existing address mapping.

## Address choice

The backing buffer reserves 1 GiB on the first pool allocation. Guest addresses
start at `0x10000`, and host addresses start at the value returned by
`vkGetBufferDeviceAddress`. Novena computes `delta = host_base - guest_base`
with wrapping subtraction. The guest range fits in 32 bits, including interior
pointers. No host address is truncated.

`VK_KHR_buffer_device_address` capture/replay does not portably select an
arbitrary guest base. A nonzero opaque address is a token previously retrieved
for an identically created buffer on the same implementation. It is not a
literal address that Novena can choose. See the
[opaque capture address specification](https://docs.vulkan.org/refpages/latest/refpages/source/VkBufferOpaqueCaptureAddressCreateInfo.html).
Using unconstrained Vulkan addresses directly could provide delta zero, but
does not guarantee that guest pointers fit in 32 bits. The arena keeps that
guarantee without relying on a driver-specific opaque address encoding.

On the tested RADV implementation, capture/replay is advertised but a 64 KiB
allocation requesting opaque memory address `0x10000` returns
`VK_ERROR_INVALID_OPAQUE_CAPTURE_ADDRESS`. That driver experiment supports the
arena fallback on this host. It does not establish an opaque address encoding
for other implementations. The details are in provenance 0022.

The arena uses ordinary bound memory, not sparse residency or a growing virtual
reservation. Its capacity is an implementation limit, not a measured guest
limit. Allocation fails if the host cannot allocate the arena or the free
ranges cannot fit a pool. Novena does not relocate live pools, change delta,
or silently introduce per-pool deltas. A per-pool fallback would require another
shader contract and is not implemented.

## Pool allocation and aliases

Pool allocations use first fit with 16-byte alignment and tail padding.
`MemoryPoolGetBufferAddress` returns the guest base for that allocation.
`MemoryPoolMap` returns the original program storage pointer.
`TextureGetTextureAddress` returns its pool's guest base plus the texture offset.
Shader code reads and buffer-to-texture CPU reads resolve GPU addresses through
the pool table before using the host memory callbacks.

Original driver returns are retained in a separate observed address field for
CPU inspection. The resolver also accepts that observed range. Recording an
original return never changes the assigned Vulkan base or delta. Original
driver addresses are not physical shader pointers in Novena's device.

A pool whose CPU storage range is contained in an existing pool shares the
same backing bytes. Its guest base includes the corresponding offset. Such an
alias must preserve 16-byte alignment. The backing block remains allocated until
the last alias is finalized, even if the original pool is finalized first.
Partial overlaps that extend a live block are rejected. Moving a block to join
overlapping ranges would invalidate guest pointers already stored in memory.

Finalization waits for device idle before releasing a range. Free neighbours
coalesce and can be reused. Reused blocks and their complete-word tail padding
start zeroed. Live addresses and delta do not change when another pool is freed.
Pointers to finalized pools are invalid, including pointers retained by textures.

These rules are Novena choices. The pool flags, virtual mapping operations,
and alias semantics of the target API are not established by the project records.
Novena does not infer those semantics from the observed flag values.

## Vulkan allocation and features

The arena buffer has `SHADER_DEVICE_ADDRESS`, `STORAGE_BUFFER`, `TRANSFER_SRC`,
and `TRANSFER_DST` usage. Its allocation includes the `DEVICE_ADDRESS` flag.
Novena binds the complete allocation before querying its buffer address. See the
[Khronos buffer device address guide](https://docs.vulkan.org/guide/latest/buffer_device_address.html).

The Vulkan context requires Vulkan 1.2, `bufferDeviceAddress`, `shaderInt64`,
and a graphics queue that also supports compute. Device selection prefers a
compatible discrete GPU, then tries the other compatible devices. This uses
the core Vulkan 1.2 form of `VK_KHR_buffer_device_address`.

The device enables `storageBuffer8BitAccess`, `storageBuffer16BitAccess`,
`shaderInt8`, and `shaderInt16` independently when supported.
`Context::create_global_shader_module` rejects modules declaring unavailable
narrow integer or storage capabilities before calling Vulkan. Its feature check
does not replace SPIR-V validation or checks for unrelated capabilities.
The feature requirements come from the
[Vulkan SPIR-V environment](https://docs.vulkan.org/spec/latest/appendices/spirvenv.html).

## Shadowbox push constants

The Shadowbox `gmem-flat` contract publishes an 8-byte `uint64` delta at byte
offset 0. Novena exports `PUSH_GLOBAL_DELTA_OFFSET = 0` and
`PUSH_GLOBAL_DELTA_SIZE = 8` in `global_memory`. `GlobalMemory::push_constant_range`
constructs that range, and `GlobalMemory::push_delta` writes the arena's delta
for the requested shader stages.

Every future draw or dispatch with global accesses must use a pipeline layout
covering this range and push the delta before execution. Other push constants
must start after byte 8. If a stage also needs another push-constant range,
combine its bytes into one range, as Vulkan permits each stage in only one
pipeline-layout push-constant range. See
[pipeline layout valid usage](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineLayoutCreateInfo.html).
The GLSL integration fixture adds its root guest pointer at byte 8.
The Shadowbox integration test embeds known guest addresses in synthetic Maxwell
MOV32I instructions and reserves only the published 8-byte delta range.

The translator uses its default nonzero-delta lowering. The arena does not
guarantee delta zero, so `global_delta_zero: true` is invalid for this backend.
Guest pointers stored in memory, descriptors, or constant buffers stay guest
addresses. Adding delta happens at each physical access, not when uploading
pointer values.

## Visibility and lifetime

The arena uses host-visible coherent memory. Queue submission uploads registered
pool storage through host callbacks in chunks of at most 64 KiB. An inaccessible
storage range stops submission with `BadArgument`. Unreadable storage and pool
flags do not establish a supported GPU-only allocation mode.

`GlobalMemory::read_pool` and `download` wait for device completion before reading
coherent backing bytes. Shader submissions must include a shader-write to
host-read memory barrier before readback. Download passes the result through the
host write callback. The integration tests exercise upload, barrier, completion,
and download. Future asynchronous queues will need explicit dependency and
ownership tracking instead of the current synchronous waits.

The backing buffer owns a shared reference to its Vulkan context and waits for
idle before destruction. Moving a `GlobalMemory` object out of its backend does
not destroy its device. Pools never migrate to another buffer or device address.

## Current execution boundary

Novena still records draws without executing shaders, as described in
[drawing.md](drawing.md). This change supplies GPU memory, address returns,
feature negotiation, module checks, and the push-constant helper. It does not
add a production graphics pipeline or dispatch implementation.

The direct Vulkan tests execute project-authored physical-address shaders through
the production allocator and delta helper. They establish the memory contract
independently of the unfinished draw path. A future shader executor must call
the helper before every draw or dispatch and download written pools after its
host-read barrier. There is no descriptor search fallback for invalid pointers.

## Evidence

[Provenance 0022](../provenance/0022-flat-global-memory.md) records the public
sources, assumptions, and verification commands. Unit tests cover wrapping
delta arithmetic, bounds, alignment, aliases, allocator reuse, and missing
narrow features. Required Vulkan tests cover API address returns, cross-pool
pointers loaded from memory, upload and download, and independent adjacent
narrow stores. Unlike the earlier optional clear tests, these tests fail when
Vulkan or their shader tools are unavailable.

The [Shadowbox integration proof](../provenance/0023-shadowbox-global-memory-proof.md)
executes 156 synthetic Maxwell programs from the sibling `gmem-flat` checkout in
the full 1 GiB arena. It checks B32, B64, B128, signed and unsigned narrow accesses,
pointer chasing across pools, and global atomics against exact whole-buffer
results. Run it explicitly with
`cargo test --features vulkan --test shadowbox_global_memory -- --ignored --nocapture`.
