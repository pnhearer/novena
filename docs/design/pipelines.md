# Shader pipelines

Novena will build Vulkan pipelines from owned snapshots of guest shader programs.
Translation output defines the shader interface. Recorded guest object addresses
identify state, while program contents identify reusable compiled code.

Stage 1 provides a compute pipeline cache and a command recording helper in
`gpu::pipelines`, behind `vulkan`. An optional `shadowbox` feature enables its
translated GPU proof. Graphics pipelines, disk persistence, background workers,
and execution of guest command-buffer dispatch records are later stages.
The existing guest queue path still executes clears and copies as before.

## Program capture and translation boundary

[Signatures 0007](../signatures/0007-program-shaders.md) and
[0008](../signatures/0008-program-shader-state.md) establish the retained shader
record. Novena stores its address, eight words, and GPU-shaped values at offsets
0 and 0x30 in the program side table. The first value resolves through registered
pools. The second value and record stride for counts above one remain unresolved.

[Provenance 0021](../provenance/0021-program-layout.md) establishes these layouts:

| Magic | Header passed to translation | Code begins |
| --- | --- | --- |
| 0x12345678 | 80 bytes at guest offset 0x30 | Guest offset 0x80 |
| 0x12345679 | 80 zero bytes | Guest offset 0x100 |

The existing reader bounds the snapshot to the pool end and 64 KiB, with a
zero-run length heuristic. That heuristic is an implementation choice, not a
known code-size field. Snapshot acquisition stays on the host-facing path.
Workers receive owned header and code bytes and never follow guest pointers.

`ShaderTranslator` receives the header followed by code and returns SPIR-V
words. Stage 1 retains this interface and derives descriptors from those words.
The separate Shadowbox adapter calls its header-prefixed translation entry point.
Shadowbox's additional `requires_subgroup_size_32` flag cannot travel through
this trait. The test adapter rejects that flag. Supporting those programs
requires retaining metadata and enabling a compatible subgroup-size contract.

Shadowbox is not a workspace dependency. The optional test creates an isolated
Cargo package under `target`, with a path dependency selected only by
`NOVENA_SHADOWBOX_PATH`. No path is built into Novena. An unset variable reports
a skip; a configured invalid path fails. Translation, validation and GPU failures
with a configured checkout fail the proof.

## Descriptor layouts

Each reflected resource contributes its set, binding, descriptor type and count.
Compute declarations use `VK_SHADER_STAGE_COMPUTE_BIT`. A graphics layout will
merge stage declarations by set and binding, require matching types and counts,
and union stage visibility. Conflicts fail layout derivation.

| Translation output declaration | Vulkan descriptor |
| --- | --- |
| Separate texture at binding 2k | SAMPLED_IMAGE |
| Separate sampler at binding 2k+1 | SAMPLER |
| Storage image at binding 2048+k | STORAGE_IMAGE |
| Uniform Block for a constant-buffer bank | UNIFORM_BUFFER |
| StorageBuffer Block, or Uniform BufferBlock | STORAGE_BUFFER |
| Physical global-memory pointer | No descriptor |

These numbers describe the translator interface, not guest API enum values.
Preserve emitted set numbers. Shadowbox's documented constant-buffer lowering
uses set 0 and binding equal to the bank. Its texture documentation establishes
set 0 for the initial texture/sampler pair. Reflection remains authoritative
for the emitted interface, including future set assignments.

Uniform banks and image resources can conflict if an output gives them the same
set and binding. Novena rejects duplicate declarations; it does not renumber
bindings without changing the shader. A translator revision must establish any
new namespace contract. The same check applies when graphics stages use the
same bank number for different resources.

Binding 2048 is one sparse binding, not 2049 descriptors. A descriptor array
contributes its actual element count. Stage 1 multiplies fixed array dimensions
with overflow checks and rejects runtime or specialization-dependent descriptor
arrays. A runtime array inside a storage-buffer block is a buffer member, not a
descriptor array. Empty intervening sets retain empty layouts so set numbers
stay unchanged.

Layout creation checks supported set layouts and device limits for set count,
each descriptor type, and per-stage resources. The inspector accepts one compute
entry point named `main`. It checks instruction lengths and supported interface
shapes. It does not replace SPIR-V validation or general capability negotiation.
The optional GPU proof runs `spirv-val --target-env vulkan1.2` before Vulkan.

Stage 1 exposes the set layouts for explicit host descriptor allocation and
updates. `GlobalMemory::uniform_buffer_info` describes a live, bounded arena
slice with uniform offset alignment and range limits checked. Its buffer now
includes UNIFORM_BUFFER usage. Later guest binding execution must resolve pool
addresses, check resource kind, populate every required descriptor element, and
retain resources through completion. Guest texture/sampler handle semantics and
stage-index mappings need further evidence.

## Push constants and specialization

The [flat-memory contract](global-memory.md) reserves bytes 0 through 7 for the
uint64 guest-to-host address delta. Stage 1 always includes that compute range.
If the translated module declares a push block, the inspector requires exactly
this member at offset 0. Extra push members fail this stage.

Every dispatch calls `GlobalMemory::push_delta` before execution. The arena keeps
one fixed delta; physical accesses add it to guest pointers. Guest pointers in
memory and uniform buffers remain guest values. Use Shadowbox's default
translation options. `global_delta_zero: true` is incompatible with an arena
that does not promise delta zero. Delta is runtime data and does not belong in
the pipeline key.

Shadowbox publishes `SPEC_ID_Y_DIRECTION = 0x5d00`. Its system-register contract
declares a float specialization constant for programs reading register 0x12,
then bitcasts that value to register bits. Future graphics compilation supplies
the value through `VkSpecializationInfo` once window-origin and coordinate
semantics are established. The float's exact bits belong in the graphics
pipeline key. Do not infer the guest origin enum or choose a sign from an
uninterpreted state value. Stage 1 uses emitted specialization defaults and has
no API to change them.

## Content cache

`ComputePipelines` fixes a translator and its configuration for the cache's
lifetime. `get_or_compile` looks up the complete normalized header and code
before invoking translation. Hash-table equality compares all bytes, so hash
collisions cannot alias programs. Identical snapshots from different allocations
share one `Arc<ComputePipeline>`. A changed header or instruction misses.
Failures remain retryable and do not enter the successful cache.

A miss translates once, reflects once, creates set and pipeline layouts, and
creates a compute pipeline through a device-local `VkPipelineCache`. The
temporary shader module is destroyed after pipeline creation. Partial failures
destroy created layouts and any partially returned pipelines. The pipeline
owns a shared context reference and survives cache destruction.

This cache does not yet replace the existing eager translation during
ProgramSetShaders or its late-read retry path. The next integration should route
both paths through one compiler service. It must retain each program's snapshot
or request handle instead of translating eagerly and translating again at
pipeline construction.

Persistent translation keys will use a stable digest of header and code plus
translator version, generation, options, entry point and interface revision.
Retain enough content identity to detect collisions. Changing translators or
options creates a new namespace. A graphics pipeline key also includes all stage
contents, specialization values, descriptor compatibility, attachment formats
and sample count, and the established static render state. Dynamic viewport and
scissor values stay outside that key. Vulkan handles and guest object addresses
never identify code contents.

## Driver cache persistence

Stage 1 owns an empty in-memory `VkPipelineCache`. Disk persistence is the next
cache stage. Driver cache data accelerates Vulkan compilation; it neither stores
Novena's content map nor promises a driver cache hit.

Use a private cache directory supplied by the host. Persist only complete blobs
returned by `vkGetPipelineCacheData`. Serialize snapshots against creation and
merge operations. Wrap the blob with a format version, length, checksum and
translation/interface revision. Write a uniquely named temporary file, flush it,
and atomically replace the destination. A torn write must leave either the old
complete file or the new one. Separate process writers use separate temporary
files; one writer owns each final cache file.

Before loading, bound file size, check the envelope and checksum, and decode
the Vulkan version-one header as little-endian bytes. Its header size is 32.
Check header version, vendor ID, device ID and pipelineCacheUUID against the
selected device. Do not reinterpret the bytes as a Rust struct. Incompatible,
truncated or corrupt files become cache misses. A checksum detects damage; it
does not make an arbitrary untrusted blob valid Vulkan initial data.
Only the application's previously retrieved driver data is eligible for reuse.
Cache I/O failure reports a diagnostic and preserves ordinary compilation.

## Background compilation

After capture, queue owned snapshots to a bounded compiler service. Its content
map tracks queued, compiling, ready and failed requests. Duplicate requests join
one in-flight result. Workers perform translation and Vulkan creation without
holding guest object-table locks or using host callbacks. Program replacement
retains a generation token so completion of an older request cannot replace the
current program.

Prewarm at ProgramSetShaders and retry when code becomes readable. Ready results
can be bound immediately. On a cold first use, preserve command order and wait
for the requested pipeline before submitting dependent work. Never substitute
a dummy shader or drop a draw to hide a miss. Background compilation reduces
work on the recording thread; it cannot guarantee that unseen shaders finish
before their first use.

Keep queue submission on the executor. Compilation does not use the queue.
A later submission owner retains pipelines, descriptor pools, resource
allocations and command pools until its fence signals. Command-pool use and
queue use need explicit external synchronization across all context owners.
Mutable access to one cache does not synchronize other owners of the context.
Shutdown drains workers and submissions before destroying their resources.

## Render-state mapping

The following limits come from signatures 0002 and
[draw state 0009](../signatures/0009-draw-state.md). They define the input facts
for a future graphics builder, not executable enum conversions.

| Recorded input | Vulkan destination after validation | Evidence still needed |
| --- | --- | --- |
| DrawArrays first and count | vkCmdDraw vertex arguments | Primitive mapping and complete draw semantics |
| Indexed draw count, address and base vertex | Index binding and indexed draw arguments | Widths for index types 1 and 2, primitive mapping |
| Instanced draw's final arguments | Instance count and first instance | Argument order and semantics |
| Vertex stream address, size and stride | Vertex buffer and binding descriptions | Attribute formats, locations and divisor meaning |
| Viewport/scissor integer origin and sizes | Dynamic viewport/scissor | Coordinate convention and depth range |
| Near/far depth values | Viewport depth interval | Floating ABI and coordinate semantics |
| Counted render targets and optional views | Attachment compatibility and rendering state | Formats, views, samples and array layout |
| Bound blend, channel, depth/stencil state | Corresponding fixed-function state | Object layouts and enum meanings |

Derive shader stage from translation output, not the guest BindProgram mask.
The mask has observed one-bit values but no established Vulkan-stage mapping.
Keep unknown state raw and return an unsupported result when execution depends
on it. No primitive, index width, attribute format, blend factor, comparison,
culling mode or origin convention is guessed in this stage.

DispatchCompute has a likely three-integer argument shape in signatures 0002.
The host-side helper takes explicit Vulkan workgroup counts. Guest dispatch
execution still needs validated units, resource binding and command ordering.

## Stage 1 execution and proof

Construct `ComputePipelines` with a context and translator, call
`get_or_compile` with the owned header/code snapshot, populate descriptors using
the returned layouts, then call the pipeline's unsafe `record_dispatch`.
The helper checks arena device identity, set count and workgroup limits. It
binds the pipeline and sets, pushes delta, dispatches, and records shader-write
to host-read visibility. Its caller owns upload visibility, image layouts,
dependencies on other GPU work, queue submission, completion and readback.
Retain the pipeline, descriptors and live pools until completion.

Run the optional proof with the selected Shadowbox crate directory exported:

```sh
cargo test --features shadowbox --test shadowbox_pipelines -- --ignored --nocapture
```

The proof translates original synthetic compute instructions and checks complete
pool-byte results for a physical-address copy and a uniform-buffer LDC followed
by a global store. It validates SPIR-V, exercises real descriptor layout creation
and binding, verifies one translation and one pipeline on a content hit, and
checks a changed-content miss. It also executes a pipeline after its cache has
been dropped. Reflection unit tests cover image/sampler types, fixed arrays,
sparse bindings, runtime buffer members and unsupported interfaces.

[Provenance 0025](../provenance/0025-compute-pipelines.md) records the sources and
verification. GPU texture and storage-image descriptor use, graphics execution,
disk cache recovery and worker scheduling require their own later proofs.
