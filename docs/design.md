# Design notes

These notes describe the intended shape of the library. They will change as work shows what is true.

## Where the library sits

A program for the original platform obtains the graphics API through a bootstrap function: it passes a function name and receives a pointer, repeats this for every function it wants, and after that calls the functions directly. This was inferred first ([note 0002](provenance/0002-functions-are-requested-by-name.md)) and then observed in a running program ([note 0005](provenance/0005-first-census.md)).

novena provides that bootstrap function and the functions behind it. A host (an emulator or a recompiled program's runtime) points the program at novena's bootstrap function, or forwards the program's calls to it.

```
program  ->  API calls  ->  novena  ->  Vulkan  ->  host GPU
                              ^
                        shader translator (separate component)
```

## What the host must supply

The interface as it stands is described in [host-interface.md](host-interface.md). In full, a host will supply:

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

1. Done: function table, instances, and a null implementation that records which functions are requested and called.
2. Done: a census from a real program. It called 168 of the 534 functions; see [docs/census](census).
3. Done for register arguments: the shape of the arguments and results of those 168 functions, and a signature entry for each, in [docs/shapes](shapes) and [docs/signatures](signatures). Still to observe: what the memory behind pointer arguments looks like.
4. Object lifetime for the objects a first frame needs.
5. A single textured triangle from a test program written for this project.
6. Presentation with a render scale.
7. Breadth, driven by the census.
