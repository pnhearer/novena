# Host interface

The host is whatever runs the program: an emulator, or the runtime of a recompiled program. novena does not load or run programs. The host tells it what the program asked for and forwards the program's calls. The C declarations are in [include/novena.h](../include/novena.h). This page describes interface version 1.

## The two moments a host hooks

1. **The program asks for a function by name.** Programs obtain the graphics API through a bootstrap function that takes a name and returns a function pointer (see [provenance note 0002](provenance/0002-functions-are-requested-by-name.md)). When the program calls it, the host calls `novena_instance_request` with the name and gets a function id back. The host then gives the program a pointer of its own making that it will recognise later, one per id.
2. **The program calls one of those pointers.** The host gathers the argument registers into a `novena_registers` and calls `novena_instance_call` with the id. On return it copies the result registers back to the program.

Function ids are positions in the function table, so they are the same in every instance and every run for a given library version.

## Registers

A call's arguments are passed as the program's calling convention has them: integer registers `x[0..8]`, the low 64 bits of floating-point registers `d[0..8]`, and the stack pointer for anything passed on the stack. novena interprets them per function. The host does not need to know any function's signature.

Results come back in `x[0]`, `x[1]` and `d[0]`.

## Memory

Many arguments are pointers into the program's memory. novena never dereferences them directly, because the program may not share the host's address space. It calls the host's `read_memory` and `write_memory` callbacks. A host whose program does share its address space can implement them as plain copies.

The callbacks can be called from any thread on which the program calls the graphics API.

## What happens today

No function has behaviour yet. `novena_instance_call` counts the call, sets the result registers to zero and returns `NOVENA_UNIMPLEMENTED`. A program that needs real results will not get far, but a host can already find out which functions a program requests and calls, and in what numbers.

`novena_instance_write_census` writes that as text:

```
# novena census: 2 of 534 functions requested, 1 called, 0 unknown names requested
# calls requested name
12 yes <function name>
0 yes <function name>
unknown 1 <a name that is not in the table>
```

The census holds names and counts only. It never contains a program's data, so it can be shared.

## Versioning

`novena_host_interface_version` returns the version the library was built with. A host compares it with `NOVENA_HOST_INTERFACE_VERSION` from the header it was compiled against and stops if they differ. The version changes whenever an existing declaration changes meaning or layout, or a function id changes.
