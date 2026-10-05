# 0008: observing the memory behind address arguments

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `PointeeShape` in `crates/novena/src/observe.rs`, `docs/shapes/0002-program-a-startup-pointees.txt`

## What was learned

A method, and the second shapes file it produced. What the file shows about individual functions is recorded in the signatures tables as they are updated.

## How

Our own design, applied to an owned program running on the platform's own implementation.

Note 0006 recorded only what the argument registers held. Many arguments are addresses, and what a function does with one depends on what is behind it: a colour, an array of object addresses, a structure, or space for a result. To tell these apart, novena now reads the first 64 bytes behind each address argument when a sampled call starts and classifies each 32-bit word as zero, a small integer, a plausible single-precision float, or other, and each 64-bit word as an address or not. When the host reports the function's return, novena reads the same 64 bytes again and counts, per word, how often they changed. A word the function changes is an output.

What is kept is per-word statistics across the sampled calls: range, up to four distinct values for small integers and floats, and counts. The bytes themselves are held only between a call and its return, on the stack of the thread making the call, and are then discarded.

What is deliberately not reported: any word that is neither a small integer nor a plausible float. Text, compressed data, shader code and addresses all fall in that class and appear only as "other" or "address".

## Confidence and open questions

- Only the first 64 bytes are looked at. Larger structures and arrays show only their start.
- A structure whose fields are 8 or 16 bits wide will be misread as 32-bit words.
- An output the function writes with the value that was already there does not count as changed.
- Memory behind an address behind an argument is not followed. Arrays of object addresses show up as rows of `address` and nothing more.
