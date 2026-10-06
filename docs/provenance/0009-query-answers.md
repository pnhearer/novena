# 0009: the device query answers

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `data/device-integers.txt`, `docs/shapes/0003-program-a-startup-answers.txt`, the `x1 -> *x2 after call` lines

## What was learned

The program asks the device for 18 integer properties at start-up, by selector, and the platform's own implementation answered each as listed in `data/device-integers.txt`. The selectors run from 0 to 0x5d; the answers are powers of two or small counts, as limits usually are: 0xf, 0x20, 0x100, 0x1000, 0x4000, 0x100000.

What each selector means is not known. An implementation that returns the same answer for the same selector satisfies this program; a program that asks a selector not in the list needs a new observation.

## How

Observation of an owned program running on the platform's own implementation, with the method of note 0008 extended: when a sampled call wrote through its third argument, novena kept the second argument (the selector) together with the 32-bit value written, one entry per distinct selector. The written value is kept only when it is not itself a program address or the low half of one, and is below 2^24.

The same mechanism fired for one other function, a barrier call, where it recorded three values under one selector. That is a coincidence of argument shape, not a query, and is left in the shapes file as is.

## Confidence and open questions

- The answers are exact for this program on the one platform configuration it ran on. Whether they vary between hardware revisions or system versions is unknown.
- Selectors the program did not ask are unknown.
- The answer is read as a 32-bit value. A 64-bit answer would show only its low half.
