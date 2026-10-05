# novena

A Vulkan implementation of a console graphics API, written from scratch.

novena lets software written against that API draw through Vulkan on an ordinary PC. It is a library with a C interface. An emulator, a recompiler or a test harness links it and forwards the API's calls to it.

This is a research and hobby project. Its purposes are interoperability, study and preservation of software people already own.

## Status

Nothing renders yet. The repository holds the project rules, the design notes and an empty library that builds. See [docs/design.md](docs/design.md) for the plan and for what is known and unknown.

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

Clean room. Behaviour is worked out from what can be observed lawfully: the calls that software a contributor owns makes, the effects those calls have, and names and facts that are already public. Proprietary development material is never used. The rules are in [CLEAN-ROOM.md](CLEAN-ROOM.md), and every piece of behaviour in the library is tied to a provenance note under [docs/provenance](docs/provenance).

If you have had access to the platform's proprietary development kit, please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening anything.

## Building

```
cargo build
cargo test
```

Rust stable is enough. Vulkan is not needed yet.

## Licence

MIT or Apache-2.0, at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE). Notes on scope and intent are in [LEGAL.md](LEGAL.md).
