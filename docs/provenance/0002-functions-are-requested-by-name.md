# 0002: functions are requested by name

- Date: 2026-10-05
- Author: Khargoosh
- Covers: the bootstrap design in docs/design.md

## What was learned

Two facts about one program. It imports a single function whose name says it is a bootstrap loader for the graphics API, and no other function of that API. And its own read-only data holds 534 strings that have the form of that API's function names.

From those two facts we infer the arrangement the design is built on: the program does not link against the API's functions one by one, but asks the bootstrap function for each by name. That inference has not been confirmed by watching a call.

## How

Observation of an owned program. Its import table and the printable strings in its own executable were listed with ordinary binary tools. Only the program's own file was read for this. No development kit material was used, and the driver's code was not examined.

## Confidence and open questions

That names are present and that one bootstrap import exists is certain for this program. That the bootstrap function takes a name and returns a function pointer is inferred from that arrangement and has not been confirmed by watching a call. The signature of the bootstrap function, and whether every program uses the same set of names, are open. The names themselves will be added with the note that introduces the function table.
