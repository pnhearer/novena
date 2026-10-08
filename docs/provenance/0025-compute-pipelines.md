# 0025: compute pipelines

- Date: 2026-10-07
- Author: Khargoosh
- Covers: gpu/pipelines.rs, uniform_buffer_info, the optional pipeline proof,
  and docs/design/pipelines.md

## Facts and sources

Program capture and normalization use Novena signatures 0007, 0008 and 0009,
and provenance 0019 and 0021. The first recorded GPU-shaped address resolves
shader memory. Graphics magic 0x12345678 selects header +0x30 and code +0x80.
Compute magic 0x12345679 selects a zero header and code +0x100.
No additional guest layout, enum or resource-handle meaning was inferred.

Shadowbox was reached only through NOVENA_SHADOWBOX_PATH. Its public
src/interface.rs exposes ShaderInput code/program_record, ShaderOutput spirv,
and requires_subgroup_size_32. Its src/lib.rs publishes the header-prefixed
entry point, default global-memory translation options, SPEC_ID_Y_DIRECTION
0x5d00, and global delta offset 0 and size 8. The selected flat-memory checkout
was revision 2ee031ad7e62a76296d1465c5438c8aeb3173803. It remained read-only.

The translator descriptor numbering requested for this integration is texture
2k, sampler 2k+1 and storage image 2048+k. Layout construction reads emitted
SPIR-V rather than computing or rewriting these numbers. Shadowbox's
docs/provenance/0007-texture-sampling.md establishes the initial set-0 pair
at bindings 0 and 1. Its docs/provenance/0007-constant-buffers.md establishes
Uniform Block declarations in set 0 with binding equal to the decoded bank.
The new GPU experiment confirms bank 3 becomes one uniform descriptor at
set 0 binding 3. The generalized image numbering is an interface requirement;
the image/sampler reflection proof uses original interface fixtures, not guest
resource observations. No new guest binding semantics were established.

Shadowbox's docs/system-registers.md documents register 0x12 as an emitted float
specialization constant with SpecId 0x5d00, bitcast to uint register bits.
The design retains that contract without assigning guest window-origin values.
Its docs/design/global-memory-flat.md supplies the explicitly requested delta
contract. Only that host contract and Mesa/envytools/public Vulkan facts inform
this work; no facts or implementation from the other projects mentioned in that
document were used.

The new synthetic LDC uses Mesa's public SM50 encoder in
src/nouveau/compiler/nak/sm50.rs at revision
7b76f8646e387c17022f8586d224738e310b5bfc:
OpLdc at lines 2619-2648 encodes opcode 0xef90, destination bits 0..7,
dynamic offset register bits 8..15, immediate offset bits 20..35, bank bits
36..40, indexed mode 0 at bits 44..45, and type bits 48..50.
set_mem_type at lines 2385-2400 maps B32 to 4.
The test uses RZ for the dynamic offset, bank 3 and byte offset 32.
MOV32I, LDG/STG, EXIT, NOP and scheduling-word encodings remain those already
recorded in [0023](0023-shadowbox-global-memory-proof.md), from envytools.
No upstream code was copied.

Public Vulkan and the linked public SPIR-V specification define the remaining
host behavior:

- [Vulkan SPIR-V environment](https://docs.vulkan.org/spec/latest/appendices/spirvenv.html)
  defines the shader environment and links the SPIR-V specification.
- [SPIR-V grammar](https://github.com/KhronosGroup/SPIRV-Headers/blob/main/include/spirv/unified1/spirv.core.grammar.json)
  defines the opcodes, operands, storage classes and decorations used by the
  interface inspector. It is a public grammar, not a guest layout.
- [Descriptor sets](https://docs.vulkan.org/spec/latest/chapters/descriptorsets.html)
  and [layout bindings](https://docs.vulkan.org/refpages/latest/refpages/source/VkDescriptorSetLayoutBinding.html)
  define type, count, binding and stage visibility.
- [Compute creation](https://docs.vulkan.org/refpages/latest/refpages/source/vkCreateComputePipelines.html)
  defines pipeline creation and partial failure handling.
- [Dispatch](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdDispatch.html)
  defines workgroup counts and required compatible descriptors.
- [Pipeline layouts](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineLayoutCreateInfo.html)
  defines push ranges and descriptor limits.
- [Uniform descriptor limits](https://docs.vulkan.org/refpages/latest/refpages/source/VkDescriptorBufferInfo.html)
  defines valid buffer offsets and ranges.
- [Cache creation](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineCacheCreateInfo.html),
  [cache header](https://docs.vulkan.org/refpages/latest/refpages/source/VkPipelineCacheHeaderVersionOne.html),
  and [cache guide](https://docs.vulkan.org/guide/latest/pipeline_cache.html)
  support the persistence design.
- [Specialization information](https://docs.vulkan.org/refpages/latest/refpages/source/VkSpecializationInfo.html)
  supports the Y-direction design.

## Implementation choices

The cache fixes a translator for its lifetime, compares complete normalized
input bytes, and looks up before translation. It caches successful Vulkan
pipelines only. Failed work remains retryable. Guest addresses are not keys.
The driver cache is in memory in this stage. Persistence and background
compilation are designs for later work.

The inspector supports one compute main entry point, separate images and
samplers, uniform/storage buffers and fixed descriptor arrays. It preserves
set numbers and sparse bindings, rejects duplicate declarations and unsupported
interfaces, and distinguishes runtime buffer members from runtime descriptors.
The only accepted push block is the published uint64 delta at offset 0.
This bounded inspector is not a complete SPIR-V validator.

Pipeline ownership uses Arc<Context> and survives cache destruction. The unsafe
recording helper leaves submission and lifetime synchronization to its caller.
It checks set count, arena device and group-count limits before recording,
pushes delta and adds compute-write to host-read visibility. The arena adds
uniform-buffer usage and exposes bounded aligned uniform descriptors.
These are host implementation choices, not observed guest API behavior.

## Experiments and limits

The optional test translates two original synthetic programs and validates their
SPIR-V with spirv-val. One copies a word through physical guest addresses. The
other loads bank 3, offset 32 through a real uniform descriptor and stores that
word into another arena pool. Both check every destination byte and sentinel.
The hit test uses a distinct byte allocation with identical contents, asserts
Arc identity and a translation count of one, then changes a load offset and
checks a second translation and distinct pipeline. Cached pipelines execute
after the cache is destroyed. Invalid descriptor counts and representable
over-limit workgroup counts are rejected before recording.

An initial test assumed u32::MAX exceeded the implementation's dispatch limit.
This device reports u32::MAX for all three axes, so that assumption was removed.
The final check derives an over-limit value only where checked addition permits
one. No huge dispatch is submitted.

Reflection unit fixtures cover all supported descriptor kinds, fixed arrays,
sparse sets/bindings, runtime members, duplicate bindings, malformed instruction
lengths, wrong execution stage and extra push members. They are interface
fragments and are never passed to Vulkan as executable modules.

This stage proves translated host compute execution, not guest command-buffer
dispatch execution or graphics. It does not prove GPU texture/image descriptor
use, subgroup-size support, disk recovery or asynchronous scheduling.

## Verification

All checks below completed with exit code 0 on 2026-10-07:

```sh
cargo fmt --all -- --check
rustfmt --edition 2021 --check crates/novena/tests/shadowbox/global_memory.rs
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --manifest-path target/shadowbox-global-memory/compute_pipeline_cache_executes_translated_program/Cargo.toml --target-dir target/shadowbox-global-memory/build --all-targets -- -D warnings
cargo build --workspace --locked
cargo build --workspace --all-features --locked
cargo test --workspace --locked
cargo test --workspace --all-features --locked -- --include-ignored --nocapture
```

Default tests passed 35 tests. The explicit all-feature run passed 48 workspace
tests with no ignored tests, including the new translated pipeline/cache proof,
the existing direct Vulkan memory tests and all 156 translated global-memory
cases. Both new translated programs passed SPIR-V validation and exact pool-byte
readback on AMD Radeon RX 9070 XT with RADV, Mesa 26.2.4-arch1.1.

Default and all-feature builds succeeded with NOVENA_SHADOWBOX_PATH unset.
The optional pipeline test explicitly reported SKIP in that configuration.
The GPU proof used the selected flat-memory checkout and did not skip.
Cargo caches, shader tools' temporary files and driver cache output stayed
under this worktree's target directory. No workspace Shadowbox dependency or
hardcoded checkout path was added.

The project-profile packet reported that this worktree is unregistered.
No active profile or candidate promotion was available. The committed source,
tests and provenance are the project evidence.
