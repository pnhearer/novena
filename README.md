# novena

A Vulkan implementation of a console graphics API, written from scratch.

novena lets software written against that API draw through Vulkan on an ordinary PC. It is a library with a C interface. An emulator, a recompiler or a test harness links it and forwards the API's calls to it.

This is a research and hobby project. Its purposes are interoperability, study and preservation of software people already own.

## Status

Early. Nothing renders yet.

What works today:

- A table of 534 function names, found in one program and since observed being requested by it.
- The host interface: a host creates an instance, tells it which names the program asks for, and forwards the program's calls.
- A census of which functions a program requested and called, as a text file that contains names and counts only.

Every call is currently counted and answered as "not implemented". That is already useful for one thing: finding out which parts of the API a real program uses, which decides the order of the work. The first such census is in [docs/census](docs/census): one program asked for all 534 functions and called 168 of them in its first four minutes.

[docs/design.md](docs/design.md) has the plan and the open questions. [docs/host-interface.md](docs/host-interface.md) explains how a host connects.

## What it is for

Software for the original platform does not talk to the graphics processor directly. It calls a graphics API, and a driver turns those calls into hardware commands. Emulators usually reproduce the hardware underneath the driver. novena takes the place of the driver.

Working at that level has practical uses:

- The host chooses the render resolution, because the API states what to draw and how large the target is.
- Depth, motion and colour buffers are visible as what they are, which modern upscalers and frame pacing need.
- There is no hardware command stream to decode.

One thing does not go away. Software for the platform ships its shaders already compiled for the original graphics processor, so those still have to be translated. That work is planned as a separate component.

## What it is not

- It is not an emulator. It has no processor, no operating system and no file formats.
- It contains no code, headers, documentation, shaders, keys, firmware or game data from the platform holder or from any game.
- It does not decrypt anything and does not get around any protection measure.
- It is not affiliated with, endorsed by or sponsored by any platform holder or hardware vendor. Names of other companies' products appear only to say what the library is compatible with.

## How it is written

Clean room. Behaviour is worked out from what can be observed lawfully: the calls that software a contributor owns makes, the effects those calls have, and names and facts that are already public. No proprietary development kit, confidential or leaked material, or proprietary code is ever used, from any party. The rules are in [CLEAN-ROOM.md](CLEAN-ROOM.md), and every piece of behaviour in the library is tied to a provenance note under [docs/provenance](docs/provenance).

If you have had access to any proprietary development kit or confidential material that relates to this area, please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening anything.

## Building

```
cargo build
cargo test
examples/run-host.sh
```

Rust stable and a C compiler are enough. Vulkan is not needed yet. The last command builds a small host in C against `include/novena.h`, runs it and prints the census it wrote.

## Licence

You choose one of three:

- **Noncommercial use:** free, under PolyForm Noncommercial 1.0.0.
- **GPL and AGPL projects:** free, under the GNU Affero GPL version 3 or later.
- **Closed commercial use:** needs a paid commercial licence.

[LICENSE.md](LICENSE.md) has a table that says which one fits your case. [FAQ.md](FAQ.md) explains the reasons in plain words, and [COMMERCIAL.md](COMMERCIAL.md) covers commercial licences. Notes on scope and intent are in [LEGAL.md](LEGAL.md).
