# Host interface

The host is whatever runs the program: an emulator, or the runtime of a recompiled program. novena does not load or run programs. The host tells it what the program asked for and forwards the program's calls. The C declarations are in [include/novena.h](../include/novena.h). This page describes interface version 5.

## Presentation

Version 5 appends `vulkan`, an optional pointer to `novena_host_vulkan`.
A null pointer keeps the existing RGBA callback path. Native presentation
requires a library built with the `vulkan` feature.

The native contract has these fields:

| Field | Contract |
|---|---|
| `extension_count`, `extensions` | Count and array of NUL-terminated instance extension names. Include `VK_KHR_surface` and the platform surface extension. Names remain valid until instance creation returns. |
| `create_surface` | Receives the Vulkan instance, window object address, and native token passed to the window builder. Returns a new `VkSurfaceKHR` as a `uint64_t`, or zero on failure. Novena owns the surface and destroys it on window finalization or instance destruction. |
| `drawable_size` | Writes the window's current pixel width and height. Either dimension may be zero while the window is hidden or minimized. |

The struct and both callbacks remain valid until instance destruction.
The native window remains valid until its window object is finalized or the
instance is destroyed. Callbacks run on the calling program thread, under the
backend lock, and must not reenter novena. The host maps guest tokens to host
windows. Novena never dereferences a guest native token.

Novena enables the requested instance extensions and the device swapchain
extension. Window initialization creates the surface and finds a presentation
queue on the selected device. Presentation creates the swapchain, blits the
selected texture into an acquired image, and presents that image. Blits handle
RGBA and BGRA channel order, sRGB encoding where required, and scaling to the
drawable extent. Resize and out-of-date results rebuild the swapchain.
A zero drawable extent skips the frame.

Two frame slots each have a command buffer, acquire semaphore, and fence.
Novena waits for a slot's fence before reusing that slot. Each swapchain image
has a separate presentation wait semaphore. Positive present intervals use
FIFO pacing. Zero requests immediate presentation, with FIFO as fallback.
Intervals above one currently use the same FIFO pacing as interval one.

The optional `wait_vblank` callback runs before either presentation path.
The RGBA callback receives the Vulkan offscreen blit's scaled rows.
`render_scale` values below 1.0 are treated as 1.0. Native presentation
uses the drawable extent. CPU fallback keeps the texture's base dimensions.

Native initialization failures return `NOVENA_INTERNAL_ERROR` and a zero
result register. A build without Vulkan returns `NOVENA_UNIMPLEMENTED` for
native window initialization. Presentation and texture transfer failures
return an error status. A host that requires native presentation must check
these statuses.

See the [cycling clear example](../examples/present.md) and
[provenance note 0026](provenance/0026-presentation.md).

## The two moments a host hooks

1. **The program asks for a function by name.** Programs obtain the graphics API through a bootstrap function that takes a name and returns a function pointer (observed, see [provenance note 0005](provenance/0005-first-census.md)). When the program calls it, the host calls `novena_instance_request` with the name and gets a function id back. The host then gives the program a pointer of its own making that it will recognise later, one per id.
2. **The program calls one of those pointers.** The host gathers the argument registers into a `novena_registers` and calls `novena_instance_call` with the id. On return it copies the result registers back to the program.

Function ids are positions in the function table, so they are the same in every instance and every run for a given library version.

## Registers

A call's arguments are passed as the program's calling convention has them: integer registers `x[0..8]`, the low 64 bits of floating-point registers `d[0..8]`, and the stack pointer for anything passed on the stack. novena interprets them per function. The host does not need to know any function's signature.

Results come back in `x[0]`, `x[1]` and `d[0]`.

## Memory

Many arguments are pointers into the program's memory. novena never dereferences them directly, because the program may not share the host's address space. It calls the host's `read_memory` and `write_memory` callbacks. A host whose program does share its address space can implement them as plain copies.

Memory callbacks may run on guest threads or on the library's execution thread.
The host context and callbacks must remain valid until instance destruction
returns. Queued submissions drain before that return. A callback must not wait
for completion on its own instance. A call that would drain its own execution
thread returns an internal error.

Guest submission copies the handle array and claims the finished recordings
before returning. Interpretation and host memory transfers run in submission
order. Memory used by submitted commands must stay valid and must not be
modified until completion. Explicit finish, presentation, and resource changes
drain earlier work. Completion reports the first pending execution error.
Validation errors clear after that report; driver failures remain fatal.
A submitted recording is consumed once. Independent command buffers record
without a shared recording mutex. A recording may move between threads when
the caller transfers ownership. Simultaneous calls on the same buffer fail
immediately rather than waiting for its current owner.

## What happens today

The set-up path has behaviour. Vulkan clears and copies update supported
base-level images in the flat arena. Vulkan presents through a native swapchain
or the offscreen RGBA callback. CPU fallback clears supported window textures
and performs supported copies. Draw commands and shader records are retained.
Draws are not executed.
Shader translation is an opt-in Rust hook, not a built-in translator. Calls
outside the implemented set are counted and return NOVENA_UNIMPLEMENTED.

`novena_instance_write_census` writes that as text:

```
# novena census: 2 of 534 functions requested, 1 called, 0 unknown names requested
# calls requested name
12 yes <function name>
0 yes <function name>
unknown 1 <a name that is not in the table>
```

The census holds names and counts only. It never contains a program's data, so it can be shared.

## Observing shapes

A host can let the platform's own implementation keep doing the work and only tell novena what passed by. It calls `novena_instance_call` before the original function, ignores what novena writes to the registers, runs the original, and then calls `novena_instance_returned` with the result registers.

novena samples the first calls of each function and records the shape of what each register held: its range, a few distinct values, and whether the values were addresses the host could read (it asks the host's `read_memory` to find out). For an address, it also classifies the first 64 bytes behind it word by word, and reads them again when the host reports the return, to see which words the function changed. `novena_instance_write_shapes` writes all of that as text.

Addresses are counted and never listed. Memory is not stored or printed verbatim: a word shows up as a small integer, a plausible float, an address, or just "other".

The before-and-after comparison relies on the host reporting a function's return on the same thread as its call, with no other reported call in between.

Pool registration also lets novena resolve observed GPU-shaped values to the
program storage behind a pool. The census and shapes reports include the
observations used for that work. This is how signatures are worked out. See
[provenance note 0006](provenance/0006-observing-shapes.md).

## Versioning

`novena_host_interface_version` returns the version the library was built with. A host compares it with `NOVENA_HOST_INTERFACE_VERSION` from the header it was compiled against and stops if they differ. The version changes whenever an existing declaration changes meaning or layout, or a function id changes.
