# 0006: observing argument and result shapes

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `crates/novena/src/observe.rs`, `novena_instance_returned`, `novena_instance_write_shapes`

## What was learned

Nothing about the graphics API yet. This note describes the method that later notes will cite when they state a function's signature.

## How

Our own design. While a program runs on the platform's own implementation, the host reports each call's argument registers to novena before the call and the result registers after it. For the first 2,048 calls of each function, novena records for every register:

- the smallest and largest value,
- up to six distinct values,
- how often the value was zero,
- how often the value was an address the host could read.

From that a reader can usually tell an object pointer from an index, a flag, a size, a constant or a floating-point value, and can tell how many arguments a function takes by where the registers stop looking like arguments and start looking like leftovers.

What is deliberately not recorded: the contents of any memory an address points to, any buffer, any image, any shader, any text. An address that was readable is counted and not listed.

## Confidence and open questions

- A register a function does not use still holds a value. Telling a constant argument from a leftover takes judgment, and sometimes a second program or a second scene.
- Shapes do not show the fields of a structure passed by pointer. For those, a later method will have to observe how the program fills the structure, again without keeping its contents.
- The sample is the first 2,048 calls. A function whose arguments change character later in a run would be misread.
