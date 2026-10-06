# Changelog

## Unreleased

- Behaviour for the set-up path: device, queue, sync, event, window, memory pool, texture, sampler, pool, program, state and command buffer management, with objects kept in a side table.
- Function table of 534 names, with lookup by name and by id.
- Instances, created by a host, that record which functions a program requests and calls.
- Census output as text.
- Host interface version 2 and a C header.
- Host interface version 3 with CPU clear recording and optional presentation.
- Sampling of argument and result shapes for hosts that let the original implementation run.
- First census from a running program.
- Sampling of the memory behind address arguments, with detection of what a call wrote, and per-selector answers of query functions.
- First shapes file, and a signature entry inferred from it for each of the 168 functions the program called.
- Example host in C.
- Project rules: clean room, contributions, licenses, scope and intent.
