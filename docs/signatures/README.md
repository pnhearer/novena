# Signatures

What each function takes and returns, as far as observation shows. Nothing here comes from documentation or headers. Every entry is an inference from a shape file under [docs/shapes](../shapes) and from the function's own name, and carries a confidence:

- **firm**: the shape leaves no reasonable alternative.
- **likely**: the shape fits and the name supports it, but another reading is possible.
- **open**: recorded so it is not forgotten; do not build on it.

Types are deliberately coarse for now:

- `object`: the address of an object the program allocated. For the first argument of most functions, the object the function name refers to.
- `int`: an integer in a general register. Whether it is 32 or 64 bits wide, signed, or an enumeration is not known unless stated.
- `float`: a single-precision value in a floating-point register.
- `pointer`: the address of other memory (an output, an array, a structure).
- `bool` as a result: the value 1 was returned every time. That it means success is an inference from the function name.

A register that looked like a leftover from an earlier call is not listed as an argument. That judgment is the main source of error in these tables.

| File | Covers |
|---|---|
| [0001-setup.md](0001-setup.md) | Device, queue, sampler, window and a few command buffer functions |
| [0002-command-buffer.md](0002-command-buffer.md) | The command buffer functions that carry most calls |
| [0003-objects.md](0003-objects.md) | State objects, memory pools, textures, pools and handles, programs, queue, sync, events and window |
| [0004-pointers.md](0004-pointers.md) | What pointer arguments point to: colours, outputs, arrays of objects, shader records |
| [0005-device-answers.md](0005-device-answers.md) | The answers the device integer query gave, per selector |
