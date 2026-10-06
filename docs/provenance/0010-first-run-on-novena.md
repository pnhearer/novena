# 0010: a program's first frame on novena

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `crates/novena/src/api/`, the claim in README.md that a program runs through its first frame

## What was learned

With the handlers in `api/` answering in place of the platform's own implementation, the program ran its whole start-up and its first frame on novena: it created its device, queue, memory pools, textures, samplers, programs and command buffers, recorded commands, submitted them, and called present. It made 39,609 calls to novena on the way, with no fault. After the present call it stopped, waiting for a frame that nothing displayed.

## How

Observation of an owned program, in the host described in note 0005, with the host switched to answer from novena: a call novena handled was returned to the program with novena's results, and a call novena did not handle returned zero. Only the resolver functions kept running on the original side, so the program received real function addresses to call.

The run lasted 120 seconds. The program's own log output and its calls stopped at 38 seconds, right after its first present. Nothing crashed.

## What this settles

- The handlers for the set-up path are consistent enough for this program to get through start-up. The program did not read anything from object memory that novena had not written, or it would have misbehaved earlier.
- The answers novena invents (handles equal to pool ids, graphics addresses equal to program addresses, texture storage of four bytes per texel per level) were accepted by this program.

## What it does not settle

- The hang after the first present is expected and is outside novena: the program waits on the platform's display path (a frame fence or vertical sync signal), which the host still owns and which no longer fires because the platform's own graphics implementation is out of the loop. A host that uses novena has to provide that signal itself. This is a requirement on the host, recorded in docs/design.md.
- Nothing was drawn. The drawing commands returned zero.
- Only one program.

## Confidence and open questions

- The point at which the program stops is inferred from the end of its log and its calls. Which wait it sits in has not been observed.
- Whether a later frame would make calls novena cannot yet satisfy is unknown.
