# Signatures 0010: remaining command state

Sources: [census 0001](../census/0001-program-a-startup.txt),
[shapes 0001](../shapes/0001-program-a-startup.txt),
[shapes 0002](../shapes/0002-program-a-startup-pointees.txt), and
[shapes 0003](../shapes/0003-program-a-startup-answers.txt).
Method, implementation choices and limits:
[provenance 0026](../provenance/0026-remaining-command-state.md).

On entry to this work, comparison with `crates/novena/src/api` gives **25
command calls** with only the generic raw recorder, which returns
`Unimplemented`, and **one resolver** with no handler. That is 26 of the 168
called functions. A handler lookup alone misses this distinction: the generic
command recorder accepts any command name. The six single-object bindings
include BindBlendState, which also appears in a state-name parsing test but
had no successful handler. Counts below come from the census, not the shorter
shape runs. Functions are written without the three-letter prefix.

## Reading the evidence

For every command below, x0 is a readable command buffer address. In shapes
0003 its pointee changes across calls: usually the address word at +0x00,
otherwise words at +0x10 or +0x20. This is firm evidence of recording state
being changed. It does not establish object sizes or an execution layout.

**Firm recording** means that the listed argument kinds can be retained in
order without interpreting their values or following unknown pointer layouts.
Labels such as stage, faces, size and handle describe likely roles. They do
not establish integer widths, signedness, enumeration meanings, units or
execution effects. Those remain likely or open even where recording is firm.
The new handlers implement only that firm recording boundary.

The census and shape files aggregate by function and do not preserve a call
sequence. Provenance [0007](../provenance/0007-first-signatures.md) describes
comparing registers in surrounding calls and matching object addresses across
setters. Signatures [0003](0003-objects.md) relate handle ids to registration
and storage sizes to later save calls. These support likely object roles,
not a new claim about which draw or dispatch follows a particular binding.
Equal call counts do not prove adjacency.

For these commands the return shape is **none seen**, with the actual return
contract **open**. Result x0 is still address-shaped, even for functions whose
x1 was an integer; there is no observed success flag or scalar result. An
address-shaped result does not distinguish a returned object from a void call
that leaves a register untouched. Result x1 and d0 may change and are not
assigned new result meanings. The table calls out the unusual ClearTexture
result separately.

## Firm recording fields

Each entry touches the command buffer's recording. Resource and state
objects below are references or inputs, not observed immediate outputs.
Except where noted, shapes 0003 sampled 2,048 calls per entry and shapes
0001 and 0002 agree on the argument classes. No register beyond the listed
fields is decoded.

| Function | Census calls | Arguments and pointer targets | Result | Confidence | Evidence and objects touched |
|---|---:|---|---|---|---|
| CommandBufferBindBlendState | 1163224 | commands: object; x1 state: object | none seen | firm recording; open layout | x1 has six or more distinct readable addresses. Its first four words are `other`, with no recorded changes. Keep a reference to blend state and any settings already known through its setters. No field offsets follow from this. |
| CommandBufferBindChannelMaskState | 135723 | commands: object; x1 state: object | none seen | firm recording; open layout | Three x1 addresses; +0x00 and +0x04 are `other`. No x1 word changed. Channel mask state is an input; later words need not belong to it. |
| CommandBufferBindColorState | 135723 | commands: object; x1 state: object | none seen | firm recording; open layout | Three x1 addresses; leading words are `other`, no changes. Some later words are addresses or small integers, insufficient to decode color state. |
| CommandBufferBindDepthStencilState | 135723 | commands: object; x1 state: object | none seen | firm recording; open layout | Two x1 addresses; +0x00 is 0x71 or 0x80, +0x04 `other`, +0x08 wide. No x1 word changed. The depth and stencil setters establish the state family, not its packed fields. |
| CommandBufferBindMultisampleState | 213906 | commands: object; x1 state: object | none seen | firm recording; open layout | Two x1 addresses; +0x00 is 5, later leading words `other`; no x1 changes. Signatures 0003 records frequent defaults calls. This does not reveal sample counts or enable bits. |
| CommandBufferBindPolygonState | 135723 | commands: object; x1 state: object | none seen | firm recording; open layout | Two x1 addresses; +0x00 is 0x14 or 0x16, +0x04 and +0x08 zero. No x1 changes. Keep polygon state settings without unpacking this word. |
| CommandBufferSetStencilMask | 135723 | commands: object; x1 selector: int; x2 value: int; no further pointer decoded | none seen | firm recording; likely faces and mask | x1 = 3 and x2 = 0xff. These differ from the object address in x1 of the state-binding family while trailing register shapes repeat. Only command state is retained; selector meanings remain open. |
| CommandBufferSetStencilRef | 135723 | commands: object; x1 selector: int; x2 value: int; no further pointer decoded | none seen | firm recording; likely faces and reference | x1 = 3 and x2 = 0; otherwise the same register classes as the mask calls. No direct write to a depth or stencil object is established. |
| CommandBufferSetStencilValueMask | 135723 | commands: object; x1 selector: int; x2 value: int; no further pointer decoded | none seen | firm recording; likely faces and mask | x1 = 3 and x2 = 0xff, matching SetStencilMask's shape. Their distinct names are retained; their effects are not equated. |
| CommandBufferBarrier | 277282 | commands: object; x1 bits: int | none seen | firm recording of x1; open synchronization and x2 role | x1 takes 0x12, 0x20, 0x30, 0x32, 0x33, 0x40. x2 is readable in 2039/2048 samples; its +0x00 changed 456 times. Shapes 0003 pairs selector 0x40 with *x2 values 1, 0, 0x100. This is not a query signature: command recording may change memory aliased by a leftover register. No output write or extra pointer argument is implemented. |
| CommandBufferSetTiledCacheAction | 5207 | commands: object; x1 action: int | none seen | firm recording; open action meaning | x1 = 2; x2 zero in 2047/2048 samples. The changed command buffer supports a recorded action, not a cache operation performed immediately. |
| CommandBufferBindUniformBuffer | 400395 | commands: object; x1 stage: int; x2 index: int; x3 location: wide value; x4 extent: int | none seen | firm recording; likely graphics address and byte size | x1 in {0,1,5}, x2 in {0,1,2}; x3 wide and unreadable; x4 spans 0x10 through 0xaa80. Location is not a CPU pointer to dereference. Likely references pool-backed buffer memory; no particular pool identity is preserved in shapes. |
| CommandBufferBindVertexBuffer | 155061 | commands: object; x1 index: int; x2 location: wide value; x3 extent: int | none seen | firm recording; likely graphics address and byte size | x1 = 0, x2 wide and unreadable; x3 in {0x100,0x1e8500,0x1e8460}. x4 has smaller varying values and is left undecoded. A vertex buffer location is retained without finding or reading its storage. |
| CommandBufferBindSeparateTexture | 209980 | commands: object; x1 stage: int; x2 index: int; x3 opaque wide value | none seen | firm recording; likely texture handle | x1 in {1,5}, x2 in 0..3; x3 wide, like the separate texture handle result in signatures 0003. It is a descriptor reference, not established as a CPU pointer. No texture or descriptor memory is changed. |
| CommandBufferBindImage | 31266 | commands: object; x1 stage: int; x2 index: int; x3 opaque int | none seen | firm recording; likely image handle | x1 = 5, x2 in 0..2, x3 spans 0x21d..0x258 in shapes 0003. Signatures 0003 relates this range to image handles. No pointee exists for x3 in the observations. |
| CommandBufferClearBuffer | 15633 | commands: object; x1 location: wide value; x2 extent: int; x3 value: int | none seen | firm recording; likely buffer address, size and fill value | x1 wide and unreadable; x2 in {4,0x8700,0x12b00}; x3 in {0,0xffffffff}. Likely refers to pool-backed memory for a later fill. Pattern width and size units are open. Recording does not write that memory. |
| CommandBufferDispatchCompute | 10422 | commands: object; x1, x2, x3: ints | none seen | firm recording; likely three group counts | x1 in {4,6}, x2 in {3,4}, x3 = 8. x4 and later registers share trailing shapes with the buffer and image calls; no extra object or pointer is decoded. A dispatch is retained without running or translating a program. Axis order and bound-program association are likely, not observed correlations. |

## Calls kept unresolved

These eight commands retain the earlier raw recording and `Unimplemented`
status. The resolver remains a host responsibility. Firm facts within an
entry do not establish enough of its contract for a successful state handler.

| Function | Census calls | Arguments and pointer targets | Result | Confidence | Evidence and objects touched |
|---|---:|---|---|---|---|
| CommandBufferBindVertexAttribState | 135723 | commands: object; x1 count: int; x2 states: pointer | none seen | likely counted state storage; open element layout | Count in {1,2,5}; four x2 addresses. +0x00 is wide in 79/2048 samples; +0x08..+0x14 are `other`. It does not show a count-sized array of object addresses. Likely inputs are vertex attribute states. Neither stride nor the address of each element can be derived. |
| CommandBufferBindVertexStreamState | 135723 | commands: object; x1 count: int; x2 states: pointer | none seen | likely inline state storage; open element stride | x1 = 1; four x2 addresses. +0x00 in {0xc,0x10,0x20,0x28}, agreeing with stride values given to setters; +0x04, +0x08, +0x0c zero. Signatures 0004 calls this the states themselves. The one-element samples do not establish a stride for larger arrays. |
| CommandBufferBindSeparateSampler | 188184 | commands: object; x1 stage: int; x2 index: int; x3 readable value | none seen | open handle versus pointer | x1 in {1,5}, x2 in 0..3, x3 five readable addresses. All sixteen words behind x3 are `other`, unchanged. This could be an opaque handle whose bits happen to be readable, or pointed-to storage. Do not dereference it or equate it with the small descriptor id. Likely touches a sampler reference. |
| CommandBufferClearTexture | 6 | commands: object; x1 texture: object; x2 null; x3 and x4 pointers; x5 int | none established; result x0 address-shaped with two distinct values | open record roles; firm pointer classifications | Six samples. x3 starts with two zero words, then wide words at +0x08 and +0x10. x4 starts with four zero words, then 1 at +0x10. Both have later addresses. x5 = 0xf. Neither pointer is established as four floats: all leading x4 words are zero. Texture, optional view, region, color and mask roles remain unresolved; no texture storage is written by the new handlers. |
| CommandBufferFenceSync | 10423 | commands: object; x1 sync: object; x2 int = 0; x3 int = 1 | none seen | likely sync reference; open condition and flags | x1 readable; +0x00 address and +0x08..+0x1c zero, no recorded changes to x1. The command buffer's +0x10 changed 2045/2048 times and +0x20 every time. Likely records a future fence, but no signalled state, timing or enum meanings are established. |
| CommandBufferSaveZCullData | 67741 | commands: object; x1 wide location; x2 int extent | none seen | likely graphics destination and size; open saved data | x2 in {0x100,0x40a0,0x9320}; x1 unreadable. Signatures 0003 relates texture storage-size results to these calls. No pointer target can be sampled. The texture association and saved format are open. |
| CommandBufferRestoreZCullData | 57325 | commands: object; x1 wide location; x2 int extent | none seen | likely graphics source and size; open restored state | x2 in {0x40a0,0x9320}, a subset of SaveZCullData sizes; x1 unreadable. Matching ranges do not prove matching locations or save-before-restore order. Likely affects later depth processing; no particular texture is named by the retained shapes. |
| CommandBufferSetPolygonOffsetClamp | 135723 | commands: object; likely d0, d1, d2 float arguments | none seen | open float count, width and order | Shapes 0001 and 0003 give d0 = 0, d1 = 0, d2 = 0x7f7fffff; later float registers vary. The third word differs from zero. x1 still looks like a state address and x2 = 0xff, fitting leftovers. None of the three proposed float values varies, so do not decode a float signature from the name alone. |
| DeviceGetProcAddress | 1070 | x0 device: object or null; x1 name: pointer to text | function pointer | firm resolver signature; open callable-address construction | 1068 non-null device arguments, two null; x1 readable; result x0 readable addresses. Provenance 0005 records reading names and receiving non-null function addresses, then calls through those addresses. Shape words alone suppress text as `other`. It touches name lookup and optionally a device, not command state. Provenance 0010 leaves real function addresses on the host side. No invented pointer result is returned. |

## Retained state

The 17 firm recording entries now have explicit records and return the same
zeroed result registers as the existing command recorder. That is novena's
host convention, not an inferred return signature. Scalar fields remain full
register-width opaque values. Single-object bindings retain the input address
and copy only settings already known through setter calls. Unknown objects
or objects of another state family have no decoded settings; program object
memory is never read to reconstruct them.

Records follow BeginRecording and EndRecording, remain in call order, and are
isolated by command buffer and recording handle. Queue submission consumes
these records without executing them. No new rendering, clearing, memory
write, synchronization or descriptor operation is performed. Pointer-array
sizes, packed state layouts, handle interpretation and execution semantics
remain open. The census coverage test leaves the eight uncertain commands
and the resolver explicitly accounted for.
