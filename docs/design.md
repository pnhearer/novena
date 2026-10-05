# Design notes

These notes describe the intended shape of the library. They will change as work shows what is true.

## Where the library sits

A program for the original platform obtains the graphics API through a single bootstrap function. It passes a function name and receives a pointer, and it repeats this for every function it wants. After that it calls the functions directly.

novena provides that bootstrap function and the functions behind it. A host (an emulator or a recompiled program's runtime) points the program at novena's bootstrap function, or forwards the program's calls to it.

```
program  ->  API calls  ->  novena  ->  Vulkan  ->  host GPU
                              ^
                        shader translator (separate component)
```

## What the host must supply

- A way to read and write the program's memory. The API passes pointers to the program's own structures, and the program is not required to be native code.
- A window or surface to present to.
- Translated shaders, or a shader translator novena can call.

The interface to the host is C, so the library can be used from any language.

## Parts

1. **Bootstrap and function table.** Name lookup, and a clear report of any name the library does not implement.
2. **Objects.** Device, queue, command buffer, memory pool, buffer, texture, sampler, program, window and the state builders. Each maps to one or more Vulkan objects.
3. **Command recording.** Calls recorded into the API's command buffers are turned into Vulkan command buffers at submission.
4. **Presentation.** The API's window object over a Vulkan swapchain, with a host-chosen render scale.
5. **Shaders.** Out of scope for this crate. A translator from the original machine code to SPIR-V is planned as its own component with its own provenance.

## Open questions

- Structure layouts, enumeration values and exact call semantics are not known. Each has to be worked out by observation and recorded in a provenance note.
- How much of the API real programs use. A census of calls from owned software will decide the order of work.
- How programs fill command buffers. If any write command memory directly instead of calling the API, this approach does not cover them.
- Whether observation alone is enough to reach a correct first frame. If it is not, the project will say so plainly.

## Order of work

1. Bootstrap, function table and a null implementation that records which functions are called.
2. Object lifetime for the objects a first frame needs.
3. A single textured triangle from a test program written for this project.
4. Presentation with a render scale.
5. Breadth, driven by the call census.
