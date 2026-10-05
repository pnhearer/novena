# Signatures 0004: what pointer arguments point to

Source: [shapes 0002](../shapes/0002-program-a-startup-pointees.txt). Method and limits: [provenance note 0008](../provenance/0008-memory-behind-arguments.md).

The earlier tables said only that an argument was a pointer. This one records what was seen behind it: the first 64 bytes, classified word by word, and which words the function changed. Function names are written without the API's three-letter prefix.

## Colours

| Function | Argument | What it points to | Confidence | Evidence |
|---|---|---|---|---|
| CommandBufferClearColor | color | four floats: red, green, blue, alpha | firm | Words +0x00 to +0x0c are floats in all 2,048 samples, with values such as 0, 0.5, 1. What follows is unrelated memory. |
| SamplerBuilderSetBorderColor | color | four floats | firm | Words +0x00 to +0x0c are each 0 or 1 in all 48 samples. |
| DeviceRegisterFastClearColor | color | four floats | firm | Words +0x00 to +0x0c take the values 0, 0.5019608 and 1. |

The channel order is assumed to be red, green, blue, alpha. Nothing observed so far distinguishes the channels.

## Outputs

| Function | Argument | What it points to | Confidence | Evidence |
|---|---|---|---|---|
| DeviceGetInteger | out | room for the result, written by the function | firm that it is an output; likely a 32-bit integer | The first 64-bit word changed across the call in 18 of 23 samples and nothing after it did. In the other 5 the value written may have equalled what was there. |
| WindowAcquireTexture | index | room for the result, written by the function | likely | The first word changed across the call in almost every sample. Together with QueuePresentTexture taking an index of 0, 1 or 2, this is taken to be the index of the acquired texture. |

## Arrays of objects

| Function | Argument | What it points to | Confidence | Evidence |
|---|---|---|---|---|
| WindowBuilderSetTextures | textures | an array of `count` object addresses | firm | With count = 3, exactly the first three 64-bit words are addresses in both samples. |
| CommandBufferSetRenderTargets | colors | an array of `count` texture object addresses | likely | The first 64-bit word is an address in 760 of 2,048 samples and the next five in about 25, which fits counts of 0, 1, 2 and 6. |
| CommandBufferSetRenderTargets | colorViews | an array of `count` view object addresses, or null | likely | When the argument is non-null its first 64-bit word is always an address. |
| CommandBufferSetRenderTargets | depth | a texture object | likely | The memory has the same layout as other texture objects seen through first arguments. |
| QueueSubmitCommands | handles | an array of `count` values, one per command buffer recording | open | With count = 1, the first 64-bit word varies. Whether a handle is an address or an opaque number is not settled. |

## Structures

| Function | Argument | What it points to | Confidence | Evidence |
|---|---|---|---|---|
| ProgramSetShaders | shaders | per shader, a record that starts with a wide value the host cannot read, then an address | likely for the two fields, open for their meaning | +0x00 is wide and unreadable in all 1,564 samples and +0x08 is always a readable address. The first is taken to be a location in graphics memory and the second a location in program memory. The rest of the first 64 bytes was mostly zero. |
| CommandBufferBindVertexStreamState | states | the state objects themselves, in an array | likely | The first word behind the pointer takes the values that were given to VertexStreamStateSetStride. |
| CommandBufferCopyTextureToTexture, CommandBufferCopyBufferToTexture, CommandBufferClearTexture | the later pointers | small records that mix zeros, small integers and object addresses | open | Too few samples, or too little structure visible, to name fields. |

## What this means for an implementation

- Colours can be read as four floats.
- `DeviceGetInteger` and `WindowAcquireTexture` must write their result through the pointer they are given.
- Arrays of objects are arrays of the program's object addresses. An implementation that keeps its own record of each object needs to find that record from the address alone.
- A shader record carries two locations. What is at those locations is the next thing to observe, and it is the hardest question in the project: the shader itself.
