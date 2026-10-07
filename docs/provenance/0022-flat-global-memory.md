# 0022: flat global memory

- Date: 2026-10-07
- Author: Khargoosh
- Covers: guest GPU address allocation, pool and texture address returns,
  CPU copy resolution, Vulkan buffer backing, feature negotiation, shader
  capability checks, and the global delta push-constant helpers

## What was learned

The pool storage pointer and size, CPU map result, GPU buffer address result,
and texture address result are recorded in
[signatures 0003](../signatures/0003-objects.md). The GPU-to-CPU offset mapping
is recorded in provenance 0016 and 0018. Those records establish the interface
shapes, not a required numeric GPU base or the meaning of pool flag bits.

Vulkan buffer device addresses require buffer usage `SHADER_DEVICE_ADDRESS`,
allocation flag `DEVICE_ADDRESS`, and enabled `bufferDeviceAddress`.
`shaderInt64` supports the shader's address arithmetic. Vulkan 1.2 includes
the core form of `VK_KHR_buffer_device_address`. Bound buffer addresses name
the buffer's complete byte range and remain valid while its backing stays live.

Opaque capture/replay addresses come from an identical earlier buffer on the
same implementation. The public contract does not specify an encoding that
allows Novena to choose an arbitrary low GPU address. Capture/replay therefore
does not establish a portable delta-zero allocation strategy for 32-bit guest
pointers.

Mesa RADV forwards an allocation's opaque capture address as its requested
replay address in `src/amd/vulkan/radv_device_memory.c:145-149`. The winsys
requests a high VA range and forwards that address to its VA allocator in
`src/amd/vulkan/winsys/amdgpu/radv_amdgpu_bo.c:544-555`. VA reservation failure
with a nonzero replay address produces `VK_ERROR_INVALID_OPAQUE_CAPTURE_ADDRESS`.
`src/amd/vulkan/radv_physical_device.c:1118` advertises capture/replay support.
These paths are relative to the permitted read-only Mesa checkout.

A project-authored Vulkan experiment on the tested RX 9070 XT created a 64 KiB
storage and device-address buffer with the capture/replay create flag. It
enabled both device-address features and requested coherent host-visible memory
with `DEVICE_ADDRESS` and `DEVICE_ADDRESS_CAPTURE_REPLAY` allocation flags.
`VkMemoryOpaqueCaptureAddressAllocateInfo::opaqueCaptureAddress` was `0x10000`.
`vkAllocateMemory` returned `-1000257000`,
`VK_ERROR_INVALID_OPAQUE_CAPTURE_ADDRESS`. This rejects that low-address request
on this host, not capture/replay in general. The experiment deliberately probes
an implementation-specific address encoding, outside the portable use of tokens
from prior captures.

The optional narrow capabilities correspond to `shaderInt8`, `shaderInt16`,
`storageBuffer8BitAccess`, and `storageBuffer16BitAccess`. Novena enables the
supported features and checks those capabilities before module creation.
The SPIR-V values used in the check are `OpCapability = 17`, `Int8 = 39`,
`Int16 = 22`, `StorageBuffer8BitAccess = 4448`, and
`StorageBuffer16BitAccess = 4433`.

The explicitly requested Shadowbox contract supplies `host = guest + delta`,
modulo 2^64, with one u64 push constant at byte offset 0 and size 8. Its
zero-delta translation option is valid only under a global zero-delta guarantee.
No instruction encoding or target ABI was inferred from another implementation.

## How

Permitted sources were the project signatures and provenance, the requested
Shadowbox contract in its `docs/design/global-memory-flat.md`,
and these public Vulkan documents:

- [Buffer device address guide](https://docs.vulkan.org/guide/latest/buffer_device_address.html)
- [Opaque capture address creation](https://docs.vulkan.org/refpages/latest/refpages/source/VkBufferOpaqueCaptureAddressCreateInfo.html)
- [Memory opaque capture address allocation](https://docs.vulkan.org/refpages/latest/refpages/source/VkMemoryOpaqueCaptureAddressAllocateInfo.html)
- [Buffer device address query](https://docs.vulkan.org/refpages/latest/refpages/source/vkGetBufferDeviceAddress.html)
- [Vulkan 1.2 features](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceVulkan12Features.html)
- [Vulkan 1.1 features](https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceVulkan11Features.html)
- [Vulkan SPIR-V environment](https://docs.vulkan.org/spec/latest/appendices/spirvenv.html)
- [Push constants](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdPushConstants.html)
- [Pipeline layout valid usage](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineLayoutCreateInfo.html)

The numeric capability values also appear in the requested Shadowbox contract's
public SPIR-V checks. No upstream memory allocator or translation implementation
was copied or translated. The GLSL fixtures and CPU tests were written for Novena.
The RADV source reads and low-address allocation probe informed the choice of
the fixed-delta arena. No Mesa implementation code was carried into Novena.

## Implementation choices and open questions

Novena reserves one ordinary 1 GiB buffer with low guest base `0x10000`.
First-fit suballocations align and pad to 16 bytes. Contained CPU storage aliases
share backing and retain it until their last reference is finalized. Partial
overlaps and unaligned aliases fail rather than move live addresses.
These are implementation choices, not established target allocation rules.

Memory comes from a host-visible coherent type. Submission mirrors all live
pool storage through bounded host reads. Inaccessible storage fails submission.
The meaning of the observed pool flags, inaccessible builder storage, and
virtual pool mappings remains unknown. The 1 GiB capacity is not a proven
guest-memory limit. A driver without the required features uses the existing
CPU backend and cannot execute physical global accesses.

Shader draw and dispatch execution remains absent from Novena's queue executor.
The delta and readback helpers are tested directly on Vulkan. Future executors
must push the delta before shader execution and use a host-read barrier before
downloading writes. A per-pool fallback is not compatible with the supplied
one-delta translator, so exhaustion fails allocation.

Original returned GPU addresses are retained separately for CPU observation
reads, including shader inspection. They do not replace an active Vulkan pool's
chosen address or change its delta. The resolver accepts either address range
for CPU reads. Observed addresses are not valid physical shader pointers in
Novena's Vulkan device.

## Verification

Run from the worktree, with temporary files and caches inside it:

```sh
mkdir -p target/tmp target/cargo-home target/cache
export TMPDIR="$PWD/target/tmp"
export CARGO_HOME="$PWD/target/cargo-home"
export XDG_CACHE_HOME="$PWD/target/cache"
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --all-features
cargo test --workspace --all-features -- --ignored --nocapture
```

The required Vulkan tests need `glslangValidator` and `spirv-val` on PATH.
They compile the two fixtures under `crates/novena/tests/shaders` for Vulkan 1.2
and validate the resulting SPIR-V before module creation. The ordinary test run
marks them ignored to keep CPU-only builders usable. The explicit ignored run
requires successful execution and compares all tested backing bytes.

The API test checks pool and texture address returns, CPU map results, CPU
resolution, aliases, finalization, reuse, and allocation failure. The two shader
tests check the shared delta across pools, a pointer read from guest memory,
unchanged neighbouring bytes, and concurrent adjacent narrow stores.

The isolated worktree is not registered with project RSI. The packet command
and terminal receipt command reported that fact. No profile was changed or
candidate promoted.

Verified on AMD Radeon RX 9070 XT with RADV, Mesa 26.2.4-arch1.1, on
2026-10-07. Formatting, strict clippy with and without Vulkan, both ordinary
workspace test configurations, and all three required Vulkan tests passed.
