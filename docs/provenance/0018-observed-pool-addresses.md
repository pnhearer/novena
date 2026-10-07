# 0018: observed pool GPU addresses

- Date: 2026-10-07
- Author: Khargoosh
- Covers: observed memory pool registration and GPU address resolution

## What was learned

In observe mode, a pool is established from the arguments to
`MemoryPoolInitialize`, whose builder identifies the storage address and
size. The GPU base for that pool is the value returned in `x0` by
`MemoryPoolGetBufferAddress`. Shader GPU-shaped values are resolved against
that observed base and then read from the corresponding program storage
address.

## How

The call arguments and return register were taken from
`docs/signatures/0003-objects.md` and the register contract in
`docs/host-interface.md`. The registration and resolution path is an own
implementation experiment using a fake host and calls replayed through the
public instance interface.

## Confidence and open questions

The pool storage and size fields and the return register are established by
the project records. The relationship between the observed GPU address and
the program storage address is still an implementation mapping; offsets are
preserved while reads use the program storage base.
