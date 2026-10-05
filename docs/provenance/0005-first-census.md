# 0005: first census from a running program

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `docs/census/0001-program-a-startup.txt`; confirms notes 0002 and 0003

## What was learned

For one program, run from start-up for four minutes with no input:

- The program asked for all 534 names in the function table, and for no name outside it.
- Each request returned a non-null function address.
- It called 168 of the 534 functions, 8,290,349 calls in total.
- 51 of the 168 are command buffer functions and they account for most of the calls. The most called function was called 1,163,224 times.
- 54 of the 168 were called 50 times or fewer. Those are the set-up calls: device, queue, memory pools, window, state objects.
- 20,846 command buffers were begun, ended and submitted, about 87 per second.

## How

Observation of an owned program, running. The program ran in a host that watches the boundary between the program and the platform's graphics library. The host saw every call to the bootstrap function, read the name passed to it from the program's memory, and noted the function address that came back. It then counted every later call the program made to one of those addresses. Each request and each call was reported to novena, which wrote the census. The original functions ran unchanged, so the program behaved normally.

Functions were recognised as secondary resolvers when their own name ends in "GetProcAddress". Whether the program used the bootstrap function or such a function for a given name was not recorded separately.

Nothing but names, addresses and counts was recorded. No arguments, no buffers and no output were kept.

## What this settles

- Note 0002 inferred that the program obtains functions by name from a resolver. That is now observed: names were passed in, addresses came back, and the program then called those addresses.
- Note 0003 inferred that the 534 strings are names the program requests. That is now observed for all 534.
- The design's open question "do programs fill command buffers by calling the API or by writing command memory directly" has a first answer for this program: it calls the API, millions of times. Direct writes would not show in a census, so this does not rule them out as well.

## Confidence and open questions

- The counts are exact for this run. Another run, or four minutes of actual play, would give different numbers and probably a few more functions.
- The 366 functions never called here may be used later in the program, by other programs, or never.
- The census says nothing about arguments or results. Working those out is the next piece of observation.
