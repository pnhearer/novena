# 0007: first signatures from observed shapes

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `docs/shapes/0001-program-a-startup.txt`, `docs/signatures/0001-setup.md`, `docs/signatures/0002-command-buffer.md`, `docs/signatures/0003-objects.md`

## What was learned

The likely arguments and results of 29 functions on the set-up path (device, queue, samplers, window) and three command buffer functions. They are tabulated, each with its evidence and a confidence, in `docs/signatures/0001-setup.md`. A second table, `docs/signatures/0002-command-buffer.md`, was read from the same shapes the same way and covers the command buffer functions that carry most of the calls. A third, `docs/signatures/0003-objects.md`, covers the rest, so that every function the program called in this run has an entry.

## How

Observation of an owned program, using the method in note 0006. The program ran for a little over three minutes on the platform's own implementation while the host reported each call's argument registers before the call and result registers after it. novena wrote the shapes file.

Each signature was then read off the shapes by hand:

- The first register is an address in almost every function, and for a given object it is the same address across the calls that configure it. That is taken as the object the function name refers to.
- A register that holds the same value as in the surrounding calls to other functions is taken as a leftover and not an argument.
- A register that takes a few small values across many calls is taken as an integer argument.
- A floating-point register whose values read as ordinary numbers (1, 4, 8; 0 and 1) is taken as a float argument.
- A result of 1 in every call of a function named "Initialize" is taken as a success flag.

Argument names in the tables come from the function names and the values. They are labels, not known parameter names.

No documentation, header or source for the API was used, and nothing was taken from memory of how the API is defined elsewhere. Where the shapes did not decide a question, the entry is marked open.

## Confidence and open questions

- Three minutes of one program with no input. Functions that appear here with one or two calls rest on very little.
- Integer widths, signedness and the meaning of each enumeration value are unknown.
- "Leftover or argument" was judged by eye. A trailing argument that happened to be constant could have been dropped.
- Structures passed by pointer are opaque. The sizes of the objects themselves (how much memory the program sets aside for a device, a builder, a sampler) are not known yet and matter, because the program allocates them.
