# Signatures 0002: command buffer functions

Source: [shapes 0001](../shapes/0001-program-a-startup.txt). Method and limits: [provenance note 0007](../provenance/0007-first-signatures.md). The reading rules are in [README.md](README.md).

Function names are written without the API's three-letter prefix and without the leading `CommandBuffer`. In every function the first argument is the command buffer object, shown as `commands`.

Two value kinds come up here that the set-up path did not have:

- `gpu address`: a 64-bit value the host could not read as program memory. In functions that bind a buffer it changes from call to call and sits where a location would. It is taken to be an address in the graphics processor's own address space, obtained earlier from a buffer or texture object.
- `handle`: a small integer that an earlier `Device...Handle` or pool registration call returned.

## Recording

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| Initialize | commands: object, device: object | bool | likely | 17,242 calls; x1 is the one device address. |
| AddCommandMemory | commands: object, pool: object, offset: int, size: int | none seen | likely | x1 one of two addresses, x2 a multiple of 0x10000, x3 = 0x10000. |
| AddControlMemory | commands: object, memory: pointer, size: int | none seen | likely | x1 readable, x2 = 0x140000. |
| BeginRecording | commands: object | none seen | likely | No register after x0 changes in step with the calls. |
| EndRecording | commands: object | open | likely | The result varies from call to call; it is probably what `QueueSubmitCommands` is later given. |
| SetMemoryCallback | commands: object, callback: pointer | none seen | open | The registers look like leftovers of the call before. From the name only. |
| SetMemoryCallbackData | commands: object, data: pointer | none seen | open | As above. |
| Finalize | commands: object | none seen | likely | Nothing but x0 is stable. |

## State binding

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| BindBlendState | commands: object, state: object | none seen | likely | x1 is an address, more than six distinct ones. |
| BindChannelMaskState | commands: object, state: object | none seen | likely | x1 one of three addresses. |
| BindColorState | commands: object, state: object | none seen | likely | x1 one of three addresses. |
| BindDepthStencilState | commands: object, state: object | none seen | likely | x1 one of two addresses. |
| BindPolygonState | commands: object, state: object | none seen | likely | x1 one of two addresses. |
| BindMultisampleState | commands: object, state: object | none seen | likely | x1 one of two addresses. |
| BindVertexAttribState | commands: object, count: int, states: pointer | none seen | likely | x1 in {1, 2, 5}, x2 one of four addresses. |
| BindVertexStreamState | commands: object, count: int, states: pointer | none seen | likely | x1 = 1, x2 one of four addresses. |
| BindProgram | commands: object, program: object or null, stages: int | none seen | likely | x1 an address or zero; x2 takes exactly the values 1, 2, 4, 8, 0x10, 0x20, so it is a set of one-bit flags. |
| SetStencilMask | commands: object, faces: int, mask: int | none seen | likely | x1 = 3, x2 = 0xff. |
| SetStencilValueMask | commands: object, faces: int, mask: int | none seen | likely | x1 = 3, x2 = 0xff. |
| SetStencilRef | commands: object, faces: int, value: int | none seen | likely | x1 = 3, x2 = 0. |
| SetPolygonOffsetClamp | commands: object, three floats | none seen | open | No integer register changes; d0 and d1 were zero, but d2 was 0x7f7fffff. Float count and order remain open; see signatures 0010. |
| SetViewport | commands: object, x: int, y: int, width: int, height: int | none seen | likely | See signatures 0001. |
| SetScissor | commands: object, x: int, y: int, width: int, height: int | none seen | likely | Same shape as SetViewport: x1 = x2 = 0, x3 and x4 are sizes. |
| SetDepthRange | commands: object, near: float, far: float | none seen | likely | See signatures 0001. |
| SetRenderTargets | commands: object, count: int, colors: pointer, colorViews: pointer or null, depth: object or null, depthView: pointer or null | none seen | likely | x1 in {0, 1, 2, 6}; x2 readable; x3 and x4 readable or zero; x5 always zero in this run. |
| SetTiledCacheAction | commands: object, action: int | none seen | likely | x1 = 2 in every call. |
| Barrier | commands: object, bits: int | none seen | likely | x1 takes small values such as 0x12, 0x20, 0x30, 0x33, 0x40. |

## Resource binding

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| BindUniformBuffer | commands: object, stage: int, index: int, buffer: gpu address, size: int | none seen | likely | x1 in {0, 1, 5}, x2 in {0, 1, 2}, x3 wide and unreadable, x4 a size such as 0x10, 0x80, 0x280. |
| BindVertexBuffer | commands: object, index: int, buffer: gpu address, size: int | none seen | likely | x1 = 0, x2 wide, x3 a size. |
| BindSeparateTexture | commands: object, stage: int, index: int, texture: handle | none seen | likely | x1 in {1, 5}, x2 in 0..3, x3 wide. |
| BindSeparateSampler | commands: object, stage: int, index: int, sampler: handle | none seen | open | x1 in {1, 5}, x2 in 0..3. x3 read as an address in this run, which a handle should not; unresolved. |
| BindImage | commands: object, stage: int, index: int, image: handle | none seen | likely | x1 = 5, x2 in 0..2, x3 small values in the range the image handle call returned. |

## Drawing and clearing

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| DrawArrays | commands: object, primitive: int, first: int, count: int | none seen | likely | x1 = 5, x2 = 0, x3 = 4. |
| DrawArraysInstanced | commands: object, primitive: int, first: int, count: int, baseInstance: int, instances: int | none seen | likely | x1 = 5, x2 = 0, x3 = 4, x4 = 0, x5 in {0x20, 0x40}. The order of the last two is a guess. |
| DrawElementsBaseVertex | commands: object, primitive: int, indexType: int, count: int, indices: gpu address, baseVertex: int | none seen | likely | x1 = 4, x2 in {1, 2}, x3 in {3, 6}, x4 wide, x5 = 0. |
| DispatchCompute | commands: object, x: int, y: int, z: int | none seen | likely | x1 in {4, 6}, x2 in {3, 4}, x3 = 8. |
| ClearColor | commands: object, index: int, color: pointer, mask: int | none seen | likely | See signatures 0001. |
| ClearDepthStencil | commands: object, depth: float, depthWrite: int, stencil: int, stencilMask: int | none seen | open | x1 = 1, x2 = 0, x3 = 0xff. The depth value was zero throughout, so the float is from the name. |
| ClearBuffer | commands: object, buffer: gpu address, size: int, value: int | none seen | likely | x1 wide, x2 a size, x3 in {0, 0xffffffff}. |

## Other

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| FenceSync | commands: object, sync: object, condition: int, flags: int | none seen | likely | x1 an address, x2 = 0, x3 = 1. |
| ReportCounter | commands: object, counter: int, buffer: gpu address | none seen | likely | x1 = 0, x2 wide. |
| SaveZCullData | commands: object, buffer: gpu address, size: int | none seen | likely | x1 wide, x2 one of three sizes. |
| RestoreZCullData | commands: object, buffer: gpu address, size: int | none seen | likely | x1 wide, x2 one of two sizes also seen in SaveZCullData. |
| SignalEvent | commands: object, event: object, two or three ints | none seen | open | x1 one of two addresses; x2 = 0, x3 = 2, x4 = 1 could be arguments or leftovers. |
| CopyBufferToTexture | commands: object, buffer: gpu address, texture: object, further pointers | none seen | open | x1 wide; x2, x3 and x4 readable. Not enough to order them. |

2026-10-08: [signatures 0010](0010-remaining-command-state.md) refines the
remaining recording fields and corrects the SetPolygonOffsetClamp d2 value.
Execution meanings and the float signature remain unresolved.

[Signatures 0011](0011-command-evidence.md) revisits the eight remaining
commands. Six now have likely state-only recording contracts. Separate
sampler binding retains an opaque reference; its handle-versus-pointer
interpretation remains open. Texture clear and polygon offset remain open.
