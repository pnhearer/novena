# Signatures 0003: state objects, memory, textures, programs, queue and window

Source: [shapes 0001](../shapes/0001-program-a-startup.txt). Method and limits: [provenance note 0007](../provenance/0007-first-signatures.md). The reading rules are in [README.md](README.md).

Function names are written without the API's three-letter prefix. `gpu address` and `handle` are explained in [signatures 0002](0002-command-buffer.md).

One pattern holds across all the state and builder objects and is worth stating once. Every `Set...` function returned its own first argument in the result register. Either these functions return the object, or they return nothing and the register is simply untouched. The shapes cannot tell those apart, so the result is recorded as "none seen" throughout.

## State objects

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| BlendStateSetDefaults | state: object | none seen | likely | 1,096 calls; nothing after x0 is consistent. |
| BlendStateSetBlendTarget | state: object, target: int | none seen | likely | x1 runs 0, 1, 2, ... x2 is 0 or 1 and may be a leftover. |
| BlendStateSetBlendFunc | state: object, four ints | none seen | likely | x1 to x4 each take a handful of values between 1 and 9. Which is source or destination, colour or alpha, is not decided. |
| BlendStateSetBlendEquation | state: object, two ints | none seen | likely | x1 and x2 each in {1, 4, 5}. |
| ChannelMaskStateSetDefaults | state: object | none seen | likely | 137 calls. |
| ChannelMaskStateSetChannelMask | state: object, target: int, r: int, g: int, b: int, a: int | none seen | likely | x1 runs 0, 1, 2, ...; x2, x3, x4 and x5 are each 0 or 1. Which flag is which channel follows the usual order only. |
| ColorStateSetDefaults | state: object | none seen | likely | 137 calls. |
| ColorStateSetBlendEnable | state: object, target: int, enable: int | none seen | likely | x1 runs 0, 1, 2, ...; x2 is 0 or 1. |
| DepthStencilStateSetDefaults | state: object | none seen | likely | 65 calls. |
| DepthStencilStateSetDepthTestEnable | state: object, enable: int | none seen | firm | x1 is 0 or 1 over 64 calls. |
| DepthStencilStateSetDepthWriteEnable | state: object, enable: int | none seen | firm | x1 is 0 or 1. |
| DepthStencilStateSetStencilTestEnable | state: object, enable: int | none seen | firm | x1 is 0 or 1. |
| DepthStencilStateSetDepthFunc | state: object, function: int | none seen | firm | x1 in {2, 3, 4, 5, 7, 8}. |
| DepthStencilStateSetStencilFunc | state: object, faces: int, function: int, reference: int, mask: int | none seen | likely | x1 in {1, 2}; x2 takes comparison-like values; x3 and x4 vary. The order of the last two is a guess. |
| DepthStencilStateSetStencilOp | state: object, faces: int, three ints | none seen | likely | x1 in {1, 2}; x2, x3 and x4 take small values. |
| PolygonStateSetDefaults | state: object | none seen | likely | 11 calls. |
| PolygonStateSetCullFace | state: object, face: int | none seen | firm | x1 in {0, 1, 2}. |
| PolygonStateSetPolygonMode | state: object, mode: int | none seen | firm | x1 in {0, 1, 2}. |
| MultisampleStateSetDefaults | state: object | none seen | likely | 305,649 calls on one of three objects. The program re-initialises this state about as often as it binds it. |
| VertexAttribStateSetDefaults | state: object | none seen | likely | 137 calls. |
| VertexAttribStateSetFormat | state: object, format: int, offset: int | none seen | likely | x1 takes format-like values (0x0a, 0x14, 0x16, 0x22, 0x25, 0x2e); x2 takes offsets 0, 8, 0x10, 0x20, 0x30, 0x40. |
| VertexAttribStateSetStreamIndex | state: object, stream: int | none seen | firm | x1 in 0..3. |
| VertexStreamStateSetDefaults | state: object | none seen | likely | 48 calls. |
| VertexStreamStateSetStride | state: object, stride: int | none seen | firm | x1 takes sizes such as 8, 0xc, 0x10, 0x18, 0x20, 0x50. |
| VertexStreamStateSetDivisor | state: object, divisor: int | none seen | firm | x1 is 0 or 1. |

## Memory pools

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| MemoryPoolBuilderSetDefaults | builder: object | none seen | likely | 178 calls. |
| MemoryPoolBuilderSetDevice | builder: object, device: object | none seen | firm | x1 is the device in all calls. |
| MemoryPoolBuilderSetFlags | builder: object, flags: int | none seen | firm | x1 in {0x92, 0xe2, 0x18a}. |
| MemoryPoolBuilderSetStorage | builder: object, memory: pointer, size: int | none seen | likely | x1 readable in 155 of 178 calls and never zero; x2 a size such as 0x100000, 0x200000, 0x400000. In the other 23 calls the memory was not readable through the host, which is unexplained. |
| MemoryPoolInitialize | pool: object, builder: object | bool | firm | Returned 1 in all 178 calls. |
| MemoryPoolFinalize | pool: object | none seen | likely | 93 calls. |
| MemoryPoolMap | pool: object | pointer | likely | Result readable in every call. |
| MemoryPoolGetBufferAddress | pool: object | gpu address | likely | Result wide and unreadable. |
| MemoryPoolGetSize | pool: object | int | likely | Result takes the sizes seen in SetStorage. |
| MemoryPoolGetFlags | pool: object | int | likely | Result 0xe2, one of the values given to SetFlags. |

## Textures

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| TextureBuilderSetDefaults | builder: object | none seen | likely | 14,760 calls. |
| TextureBuilderSetDevice | builder: object, device: object | none seen | firm | x1 is the device. |
| TextureBuilderSetFlags | builder: object, flags: int | none seen | firm | x1 in {0, 4, 8, 9, 0xc, 0x20}. |
| TextureBuilderSetTarget | builder: object, target: int | none seen | firm | x1 in {1, 2, 4, 8, 0xa}. |
| TextureBuilderSetFormat | builder: object, format: int | none seen | firm | x1 takes values such as 0x0b, 0x0c, 0x16, 0x25, 0x27, 0x9a. |
| TextureBuilderSetLevels | builder: object, levels: int | none seen | firm | x1 from 1 to 9 and above. |
| TextureBuilderSetSize2D | builder: object, width: int, height: int | none seen | firm | x1 and x2 take powers of two and screen sizes. |
| TextureBuilderSetSize3D | builder: object, width: int, height: int, depth: int | none seen | likely | x1, x2 and x3 all take size-like values. |
| TextureBuilderSetSize1D | builder: object, size: int | none seen | open | 13,281 calls on only four builders, with x1 varying. Called far more than a one-dimensional size would suggest; it may be how the program sets another quantity. |
| TextureBuilderSetStride | builder: object, stride: int | none seen | likely | x1 in {0x3c0, 0x780} (960 and 1920). |
| TextureBuilderSetSwizzle | builder: object, r: int, g: int, b: int, a: int | none seen | likely | x1 = 2, x2 in {2, 3}, x3 in {2, 4}, x4 in {2, 5}. |
| TextureBuilderSetDepthStencilMode | builder: object, mode: int | none seen | likely | x1 is 0 or 1. |
| TextureBuilderSetStorage | builder: object, pool: object, offset: int | none seen | likely | x1 an address, x2 an offset that rises in steps of 0x200. |
| TextureBuilderGetStorageSize | builder: object | int | likely | Result takes sizes from 0x200 up. |
| TextureBuilderGetStorageAlignment | builder: object | int | likely | Result in {0x20, 0x200, 0x10000}. |
| TextureInitialize | texture: object, builder: object | bool | firm | Returned 1 in all 13,796 calls. |
| TextureFinalize | texture: object | none seen | likely | 13,369 calls. |
| TextureGetFlags | texture: object | int | likely | Result takes the values given to SetFlags. |
| TextureGetTarget | texture: object | int | likely | Result takes the values given to SetTarget. |
| TextureGetLevels | texture: object | int | likely | Result takes the values given to SetLevels. |
| TextureGetSamples | texture: object | int | likely | Result 0 in every call. |
| TextureGetTextureAddress | texture: object | gpu address | likely | Result wide. |
| TextureGetViewOffset | texture: object, view: object | int | likely | x1 one of five addresses; result a multiple of 0x200. |
| TextureGetZCullStorageSize | texture: object | int | likely | Result takes the sizes later given to SaveZCullData. |
| TextureViewSetDefaults | view: object | none seen | likely | 153,061 calls. |
| TextureViewSetLevels | view: object, base: int, count: int | none seen | likely | x1 from 0 upward, x2 is 0 or 1. |
| TextureViewSetLayers | view: object, base: int, count: int | none seen | likely | x1 and x2 are each 0 or 1. |

## Pools and handles

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| TexturePoolInitialize | pool: object, memory: object, offset: int, count: int | bool | open | One call: x1 an address, x2 = 0x20000, x3 = 0x80000, x4 = 4. Which of the numbers are arguments and what they mean is not settled. |
| SamplerPoolInitialize | pool: object, memory: object, offset: int, count: int | bool | open | One call: x1 an address, x2 = 0, x3 = 0x1000, x4 = 4. |
| TexturePoolRegisterTexture | pool: object, id: int, texture: object, view: pointer or null | none seen | likely | x1 counts up from 0x100; x2 an address; x3 zero in this run. |
| TexturePoolRegisterImage | pool: object, id: int, texture: object, view: pointer or null | none seen | likely | Same shape as RegisterTexture, 13 calls. |
| SamplerPoolRegisterSampler | pool: object, id: int, sampler: object | none seen | firm | x1 counts up from 0x100 over 48 calls; x2 a different address each time. |
| DeviceGetSeparateTextureHandle | device: object, id: int | handle | likely | x1 takes the ids given to RegisterTexture; result wide. |
| DeviceGetSeparateSamplerHandle | device: object, id: int | handle | likely | x1 takes the ids given to RegisterSampler. |
| DeviceGetImageHandle | device: object, id: int | handle | likely | The result equals x1 in the values shown, so the register may be untouched and the handle returned some other way. |
| CommandBufferSetTexturePool | commands: object, pool: object | none seen | likely | One call. |
| CommandBufferSetSamplerPool | commands: object, pool: object | none seen | likely | One call. |
| CommandBufferSetShaderScratchMemory | commands: object, pool: object, offset: int, size: int | none seen | likely | One call: x1 an address, x2 = 0x140000, x3 = 0x60000. |

## Programs

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| ProgramInitialize | program: object, device: object | bool | firm | x1 is the device; returned 1 in all 1,564 calls. |
| ProgramSetShaders | program: object, count: int, shaders: pointer | bool | likely | x1 = 1 in every call, x2 readable; returned 1. What the pointed-to data holds is the central open question for shaders. |

## Queue, sync, events and window

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| QueueSubmitCommands | queue: object, count: int, handles: pointer | none seen | likely | x1 = 1, x2 readable and different each time. |
| QueueFlush | queue: object | none seen | likely | Registers match the SubmitCommands call before it. |
| QueueFinish | queue: object | none seen | likely | 7 calls. |
| QueuePresentTexture | queue: object, window: object, index: int | none seen | likely | x1 one address, x2 in {0, 1, 2}, matching the three textures given to the window. |
| QueueWaitSync | queue: object, sync: object, timeout: int | int | open | x1 an address; x2 varies widely. Returned 1. |
| SyncInitialize | sync: object, device: object | bool | firm | x1 is the device; returned 1. |
| SyncFinalize | sync: object | none seen | likely | 8,623 calls. |
| SyncWait | sync: object, timeout: int | int | likely | x1 is zero or wide. Returned 0. |
| EventBuilderSetStorage | builder: object, pool: object, offset: int | none seen | likely | Two calls: x1 an address, x2 in {0, 0x100}. |
| EventInitialize | event: object, builder: object | bool | likely | Two calls; returned 1. |
| EventGetValue | event: object | int | likely | Result is 0 or 1. |
| EventSignal | event: object, two ints | none seen | open | x1 = x2 = 0. Could be arguments or leftovers. |
| WindowAcquireTexture | window: object, sync: object, index: pointer | int | likely | x1 and x2 readable; returned 0 every time. The acquired index is probably written through x2. |
| WindowGetPresentInterval | window: object | int | likely | Result in {1, 2}. |
| WindowFinalize | window: object | none seen | likely | One call. |
| DeviceGetProcAddress | device: object or null, name: pointer | pointer | firm | 1,070 calls. x1 readable; the result is a function address. The host uses this function as a resolver and saw the names (note 0005). x0 was zero in two calls. |
| CommandBufferGetCommandMemorySize | commands: object | int | likely | Returned 0x10000, the size given to AddCommandMemory. |
| CommandBufferClearTexture | commands: object, texture: object, further arguments | none seen | open | 6 calls: x1 an address, x2 zero, x3 and x4 addresses. |
| CommandBufferCopyTextureToTexture | commands: object, source: object, further arguments | none seen | open | 7 calls with every register constant. |
