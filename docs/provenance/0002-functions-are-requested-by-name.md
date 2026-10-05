# 0002: functions are requested by name

- Date: 2026-10-05
- Author: Khargoosh
- Covers: the bootstrap design in docs/design.md

## What was learned

A program does not link against the graphics API's functions one by one. It imports a single bootstrap function from the platform's system library and keeps the names of the API functions it wants as text in its own read-only data. In the one program examined, 534 such names are present.

## How

Observation of an owned program. Its import table and the printable strings in its own executable were listed with ordinary binary tools. Only the program's own file was read for this. No development kit material was used, and the driver's code was not examined.

## Confidence and open questions

That names are present and that one bootstrap import exists is certain for this program. That the bootstrap function takes a name and returns a function pointer is inferred from that arrangement and has not been confirmed by watching a call. The signature of the bootstrap function, and whether every program uses the same set of names, are open. The names themselves will be added with the note that introduces the function table.
