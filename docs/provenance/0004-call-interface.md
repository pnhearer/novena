# 0004: how calls reach the library

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `Registers`, `novena_instance_call`, `docs/host-interface.md`

## What was learned

Nothing about the graphics API itself. This note records why the host interface passes raw argument registers.

## How

Public fact and our own design. Programs for the platform are 64-bit Arm code, and the standard procedure call convention for that architecture is published by Arm: the first eight integer or pointer arguments travel in general registers, the first eight floating-point arguments in floating-point registers, the rest on the stack, and results come back in the first registers of each kind.

Because no function signature is known yet, the host interface does not encode any. The host hands over the registers as they are and the library will interpret them function by function as each one is worked out.

## Confidence and open questions

That the convention is the standard one is an assumption until a call is observed. It is the convention the platform's compilers use for ordinary C functions, and the functions are obtained as ordinary function pointers, so it is a strong assumption. A function that returns a large structure or takes vector arguments would use parts of the convention the `Registers` structure does not carry yet (the indirect result register and the full vector registers). If one turns up, the structure grows and the host interface version changes.
