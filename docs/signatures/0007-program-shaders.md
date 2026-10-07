# Signatures 0007: program shaders

Source: one shapes census from a 120 second observed run, 1,554 calls of
`ProgramSetShaders` and 1,554 of `ProgramInitialize`, all sampled, with the
words behind each readable pointer compared before and after the call.
`CommandBufferBindProgram` was called 205,235 times in the same run.

## ProgramSetShaders

| Register | Observed shape |
|---|---|
| x0 | An address with at least six distinct values. Its pointee changes during the call at +0x00 (794 calls), +0x10 to +0x1c (every call) and +0x20 to +0x24 (868 calls). +0x08 holds an address. |
| x1 | Always 1. |
| x2 | An address with one distinct value, pointing at a record that does not change during the call. |
| x3 | A number: 0, 1, or one large value. |
| x4 | Zero in 520 calls, readable memory in 1,033. Every word behind it fell outside the known shapes. |

The record behind x2, by offset:

| Offset | Shape |
|---|---|
| +0x00 | A wide value in every call, never a host address |
| +0x08 | An address |
| +0x10 to +0x1c | Zero |
| +0x20 | An address |
| +0x28 | Always 2 |
| +0x2c | Zero |
| +0x30 | A wide value in every call, never a host address |
| +0x38 | An address |

## What this establishes

This extends [0004](0004-pointers.md), which named the first wide value and
the address after it.

x0 is the program object, since its pointee is what the call writes. x1 is
a count, and with a count of one the record behind x2 describes one shader
stage. The wide values at +0x00 and +0x30 have the shape of GPU addresses
(compare provenance 0012: a GPU address is the program address of pool
storage, never a host address).

## What it does not establish

- Which of the two wide values is the shader code and which is other data.
- The record size: the layout fits either one record of at least 0x40 bytes
  or two records starting at +0x00 and +0x30, and the count of 1 argues for
  the first, but no observation varies the count.
- Any code size field. None of the observed small fields varies.
- The meaning of x3 and x4.

Further observation needed: calls with a count other than 1, a census of the
small values at each offset when the count varies, and a dump with offsets
labelled of the memory at the two wide values (first 0x80 bytes, shape only)
to tell code from control data.
