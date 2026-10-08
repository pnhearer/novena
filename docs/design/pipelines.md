# Shader pipelines

Novena will build Vulkan pipelines from owned snapshots of guest shader programs.
Translation output defines the shader interface. Recorded guest object addresses
identify state, while program contents identify reusable compiled code.

The compute service provides content caching, optional disk persistence, a
bounded background compiler pool and a command recording helper in
`gpu::pipelines`, behind `vulkan`. An optional `shadowbox` feature enables its
translated GPU proof. Graphics pipelines and execution of guest command-buffer
dispatch records are later stages.
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
lifetime. Its synchronous `get_or_compile` API is for prewarming and tests.
The frame-facing service is `AsyncComputePipelines`. Both compare the complete
normalized header and code before translation. Identical snapshots from
different allocations share one result. Changed bytes miss. Hash collisions
cannot alias programs because memory keys compare all bytes and disk records
retain and compare the original input.

A miss loads cached SPIR-V or translates, reflects the interface, creates
layouts and creates a compute pipeline through a device-local `VkPipelineCache`.
The temporary shader module is destroyed after pipeline creation. Partial
failures destroy created layouts and any partially returned pipelines.
A pipeline owns a shared context reference and survives cache or pool destruction.

The host opts into persistence with a private cache directory and a
`TranslationIdentity`. The version must be nonempty and identify the translator
revision. Configuration must identify generation and every option affecting
output. The namespace includes those length-prefixed strings, the compute
`main` entry point and interface revision, the disk format revision, Vulkan
deviceUUID, driverUUID, pipelineCacheUUID, vendor ID and device ID.
A BLAKE3 digest names the namespace and each program record.
Changing translator version, configuration or device/driver identity starts a
separate cache. Guest addresses and Vulkan handles never identify code.

Each translation record contains the full normalized input followed by
little-endian SPIR-V words. Loading compares input identity, reflects the module
again and creates a fresh Vulkan pipeline. If a disk translation fails reflection
or pipeline creation, the service retries fresh translation and replaces the
record after successful creation. Failed fresh translations are never persisted.
A translation hit avoids the translator; it does not reuse a Vulkan handle
across processes.

This service does not yet replace eager translation during ProgramSetShaders
or its late-read retry path. Guest dispatch and graphics execution still need
established binding and state contracts. A future integration must capture
once, request compilation through this service, and retain the request handle
instead of translating twice. A graphics key will also need all stage contents,
specialization values, descriptor compatibility, attachment formats, sample
count and established static render state. Dynamic viewport and scissor values
stay outside that key.

## Driver cache persistence

Driver cache data accelerates Vulkan compilation. It neither stores Novena's
content map nor guarantees a driver cache hit. Both cache kinds use an envelope
with an eight-byte kind tag, a little-endian format version, namespace digest,
64-bit payload length and BLAKE3 payload checksum. Reads are bounded to 64 MiB.
Unknown versions, wrong namespaces, invalid lengths and damaged checksums become
misses with diagnostics. A checksum detects damage, not hostile modification.
Only private application-owned cache files are eligible for loading.

The driver loader additionally decodes the Vulkan version-one header as
little-endian bytes. It requires header size 32, header version 1, and matching
vendor ID, device ID and pipelineCacheUUID. It never casts file bytes to a Rust
structure. Driver cache creation retries with empty data if loading fails or
the driver rejects the accepted blob.

One mutex serializes Vulkan creation and `vkGetPipelineCacheData` on the shared
driver cache. Translation and file I/O happen outside this mutex. Successful
compilation saves the translation and a complete driver snapshot on the worker.
No cache I/O runs during request submission or polling. The synchronous prewarm
API performs that same I/O on its caller and belongs outside frame recording.

Writers create unique temporary files in the destination directory with exclusive
creation, write and flush the entire envelope and payload, then atomically rename
the file and flush the directory. Readers open only final filenames. An abandoned
temporary file is ignored. Concurrent threads and processes use distinct
temporary files. The last complete rename wins for a shared destination.
Driver snapshots are not merged across processes, so concurrent writers may lose
warmup coverage while preserving a valid complete file. Translation records for
different content have separate destinations. Cache I/O failure preserves
ordinary compilation and reports a bounded diagnostic list through
`take_diagnostics`, collected away from the frame path.

## Background compilation

Construct `AsyncComputePipelines` from an in-memory or persistent
`ComputePipelines`, a positive worker count and a positive queue capacity.
Construction loads the driver cache and starts workers before frame recording.
Ready prewarmed pipelines transfer into the service. Queue capacity bounds
waiting jobs; worker count bounds active jobs.

After capture, `request` queues an owned snapshot with nonblocking `try_send`.
Its caller owns the content map, so workers never lock that map or guest object
tables. Duplicate requests return handles to one shared result.
`PipelineRequest::poll` reports queued, compiling, ready or failed. Polling uses
an atomic phase and a nonblocking result-lock attempt. Workers translate and
create Vulkan objects without host callbacks, guest pointers, queue submission
or command-pool use. Translator panics become failed requests and the worker
continues. Failed requests stay visible until explicit `retry_failed`; this
prevents a failing program from triggering compilation every frame.

On a cold first use, the frame polls once and skips the draw if its pipeline is
queued, compiling, failed, or cannot be queued because the bounded queue is full.
It continues recording the frame's other commands without waiting for the
missing pipeline. Queue-full submission leaves no cached request and can retry
on the next frame. A ready pipeline can be bound immediately. A skipped draw is
not replayed later in that frame. This is a host design choice, not observed
guest behavior or a claim of equivalent output. Measure missing-draw effects
and compilation latency later before choosing a final rendering policy.

A program replacement retains its new request handle. An old completion only
updates its own handle, so it cannot replace the current program. Future guest
integration should prewarm at ProgramSetShaders and retry capture when code
becomes readable. Graphics execution and that guest integration remain later
stages; the current guest path still skips draws.

Queue submission stays on the executor. A submission owner retains pipelines,
descriptor pools, resource allocations and command pools until its fence
signals. Queue and command-pool synchronization remain the caller's responsibility.
Pool destruction closes the work queue, drains queued jobs and joins every worker.
Do this outside frame recording. Submitted resources still require their own
completion tracking before destruction.

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

## Execution and proof

For synchronous prewarming, construct `ComputePipelines` with a context and
translator and call `get_or_compile` with the owned header/code snapshot.
For frame recording, convert that cache to `AsyncComputePipelines`, request the
snapshot and use only a ready result. Skip the draw while its result is
unavailable. Populate descriptors using the ready pipeline's layouts, then call
its unsafe `record_dispatch`.
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
checks a changed-content miss. It compiles the uniform-buffer pipeline on the
worker pool, verifies reuse and executes it after the pool has been dropped. Reflection unit tests cover image/sampler types, fixed arrays,
sparse bindings, runtime buffer members and unsupported interfaces.

[Provenance 0025](../provenance/0025-compute-pipelines.md) records the sources and
verification. GPU texture and storage-image descriptor use and graphics
execution still require later proofs. Persistent cache recovery and
worker scheduling have separate original host tests:

```sh
cargo test --features vulkan --test pipeline_cache -- --include-ignored --nocapture
```

[Provenance 0026](../provenance/0026-persistent-pipelines.md) records the disk and
background-compilation sources, experiments and limits.
