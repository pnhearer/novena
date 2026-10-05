# Signatures 0001: set-up path and a few drawing calls

Source: [shapes 0001](../shapes/0001-program-a-startup.txt). Method and limits: [provenance note 0007](../provenance/0007-first-signatures.md).

Function names are written without the API's three-letter prefix.

## Device

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| DeviceBuilderSetDefaults | builder: object | none seen | likely | One call. x0 is an address; x1 and x2 hold the same addresses as in the calls around it. |
| DeviceBuilderSetFlags | builder: object, flags: int | none seen | firm | x0 is the builder from the call before; x1 = 0x100. |
| DeviceInitialize | device: object, builder: object | bool | firm | x0 is a new address, x1 is the builder; returned 1. |
| DeviceSetDepthMode | device: object, mode: int | none seen | likely | x0 is the device; x1 = 1 in the one call. |
| DeviceSetWindowOriginMode | device: object, mode: int | none seen | likely | x0 is the device; x1 = 1 in the one call. |
| DeviceGetInteger | device: object, which: int, out: pointer | open | likely | 23 calls. x0 constant, x1 ranges over 0..0x5d, x2 is a different readable address each time. The result register varies and may be a leftover of the value written. |
| DeviceRegisterFastClearColor | device: object, color: pointer, format: int | bool | likely | 10 calls. x1 readable, x2 takes four small values, returned 1. What x2 selects is a guess. |
| DeviceRegisterFastClearDepth | device: object, format: int, depth: float | bool | open | 2 calls. d0 was 0 and 1; x1 readable, x2 = 4. The order of the integer arguments is not clear. |

## Queue

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| QueueBuilderSetDefaults | builder: object | none seen | likely | One call; x0 address. x1 = 0x2010 could be an argument or a leftover. |
| QueueBuilderSetDevice | builder: object, device: object | none seen | firm | x0 the builder, x1 the device. |
| QueueBuilderSetControlMemorySize | builder: object, size: int | none seen | firm | x1 = 0x10000. |
| QueueBuilderSetCommandFlushThreshold | builder: object, size: int | none seen | firm | x1 = 0x5000. |
| QueueInitialize | queue: object, builder: object | bool | firm | x0 new address, x1 the builder; returned 1. |

## Samplers

48 sampler builders were filled in and 48 samplers created, so these shapes rest on 48 calls each.

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| SamplerBuilderSetDefaults | builder: object | none seen | likely | x0 address. x1 is also an address in every call and may be a leftover. |
| SamplerBuilderSetDevice | builder: object, device: object | none seen | firm | x1 is the one device address in all 48 calls. |
| SamplerBuilderSetMinMagFilter | builder: object, min: int, mag: int | none seen | likely | x1 in {2, 3, 5}, x2 in {0, 1}. Which is min and which is mag follows the name order only. |
| SamplerBuilderSetWrapMode | builder: object, s: int, t: int, r: int | none seen | likely | x1, x2 and x3 each in {1, 5, 7}. |
| SamplerBuilderSetMaxAnisotropy | builder: object, value: float | none seen | firm | d0 in {1, 4, 8}. |
| SamplerBuilderSetCompare | builder: object, mode: int, function: int | none seen | likely | One call: x1 = 1, x2 = 2. |
| SamplerBuilderSetBorderColor | builder: object, color: pointer | none seen | likely | x1 readable in every call. |
| SamplerBuilderSetLodBias | builder: object, bias: float | none seen | open | d0 was zero in every call, so the float argument is inferred from the name alone. |
| SamplerBuilderSetLodClamp | builder: object, min: float, max: float | none seen | open | No varying values seen. Inferred from the name alone. |
| SamplerInitialize | sampler: object, builder: object | bool | firm | x0 a different address each time, x1 a builder; returned 1. |

## Window

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| WindowBuilderSetDefaults | builder: object | none seen | likely | Two calls; x0 address. |
| WindowBuilderSetDevice | builder: object, device: object | none seen | firm | x1 is the device. |
| WindowBuilderSetNativeWindow | builder: object, window: pointer | none seen | likely | x1 readable, same in both calls. |
| WindowBuilderSetTextures | builder: object, count: int, textures: pointer | none seen | likely | x1 = 3, x2 readable. |
| WindowInitialize | window: object, builder: object | bool | firm | Returned 1. |
| WindowSetPresentInterval | window: object, interval: int | none seen | likely | x1 = 2 in both calls. The program runs at 30 frames a second on a 60 Hz display, which fits. |

## Command buffer

These are from thousands of sampled calls each.

| Function | Arguments | Result | Confidence | Evidence |
|---|---|---|---|---|
| CommandBufferSetViewport | commands: object, x: int, y: int, width: int, height: int | none seen | likely | x1 = x2 = 0 always; x3 and x4 take values such as 0x500 and 0x2d0 (1280 by 720), 0x780 and 0x438 (1920 by 1080). All integers. |
| CommandBufferSetDepthRange | commands: object, near: float, far: float | none seen | likely | d0 = 0 always, d1 in {0, 1}. |
| CommandBufferClearColor | commands: object, index: int, color: pointer, mask: int | none seen | likely | x1 in {0, 1}, x2 readable, x3 = 0xf always (four bits, one per channel). |
