# novena

A Vulkan implementation of a console graphics API, written from scratch.

novena lets software written against that API draw through Vulkan on an ordinary PC. It is a library with a C interface. An emulator, a recompiler or a test harness links it and forwards the API's calls to it.

This is a research and hobby project. Its purposes are interoperability, study and preservation of software people already own.

## Status

Early. Clears and presentation work. A Vulkan experiment executes a first
non-indexed draw with translated shaders and explicit host format choices.
General draw support remains open.

What works today:

- A table of 534 function names, found in one program and since observed being requested by it.
- The host interface: a host creates an instance, tells it which names the program asks for, and forwards the program's calls.
- A census of which functions a program requested and called, as a text file that contains names and counts only.
- Census and shapes observation reports.
- A C host example that resolves functions, submits a clear, presents a frame,
  and writes census and shapes reports.
- A panic-guarded C interface with novena_last_error, plus CMake and pkg-config files in release archives.
- CPU texture copies with four bytes per base-level texel, 64 KiB host reads, no format decoding, and a 4096 by 4096 texel limit.
- Render scaling and optional vblank pacing through the host interface.
- CPU clears for window-presented textures and an optional Vulkan backend for arena-backed color and depth clears, supported texture copies, and native swapchain or offscreen presentation.
- Recorded draw commands, 17 documented state-only command handlers, and retained shader records.
- An opt-in Vulkan first draw with one float4 vertex attribute, one RGBA8 target,
  translated vertex and fragment stages, and disabled depth, stencil, blend,
  and culling. See [drawing support](docs/design/drawing.md).
- GPU address resolution through registered memory pools, including pools learned from observed calls.
- An opt-in `ShaderTranslator` hook that receives bounded shader bytes during shader setup and retains translated words or error counts.
- An off-by-default translated-shader dump for local debugging. A host can set
  a dump directory; successful translations are written as SPIR-V files and
  failures as one-line error files. The output is derived from the observed
  program's shaders, and the original shader bytes are never written.
- Private disk pipeline caches and bounded background compilation for compute
  and the first draw. A Rust host configures graphics persistence explicitly.
- CI coverage for the optional Vulkan feature.

About 130 functions, the set-up path, have behaviour. Novena keeps a record of
the program's objects and answers queries from observed data. One program runs
on novena alone, presenting clears without executed draws
([notes 0010 and 0011](docs/provenance/0011-running-past-the-first-frame.md)).
Draw commands and shader records are retained. Supported first draws execute
only when a Rust host supplies the explicit experiment contract. A draw whose
pipeline is still compiling is skipped for that frame; other commands continue.
Shader translation requires a registered translator and an enabled hook.
Supported base-level Vulkan texture copies and the bounded first draw path work. General resource
binding and draw execution remain open. Other calls are counted and answered
as "not implemented".

The census of what a real program calls is in [docs/census](docs/census): one program asked for all 534 functions and called 168 of them in its first four minutes.
The register observations that support the current signatures are in docs/shapes.

The first signatures worked out from observation are in [docs/signatures](docs/signatures).

[docs/design.md](docs/design.md) has the plan and the open questions. [docs/host-interface.md](docs/host-interface.md) explains how a host connects.

## What it is for

Software for the original platform does not talk to the graphics processor directly. It calls a graphics API, and a driver turns those calls into hardware commands. Emulators usually reproduce the hardware underneath the driver. novena takes the place of the driver.

Working at that level has practical uses:

- The host chooses the render resolution, because the API states what to draw and how large the target is.
- Depth, motion and colour buffers are visible as what they are, which modern upscalers and frame pacing need.
- There is no hardware command stream to decode.

One thing does not go away. Software for the platform ships its shaders already compiled for the original graphics processor. novena can pass retained shader bytes to an opt-in translator hook, but it does not provide a translator. The optional compute pipeline helper and
first draw path can execute supported translated shaders.

The optional dump directory is for local debugging only. It writes translated
output derived from the observed program's shaders, never the original shader
bytes.

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

Rust stable and a C compiler are enough. The CPU implementation is the default. To compile the optional Vulkan backend, use `cargo build --features vulkan`. Vulkan is optional at runtime too: when no loader or device is available, novena keeps using its CPU path. The last command builds a small host in C against `include/novena.h`, runs it and prints the census it wrote.

Release archives also include a CMake package and a pkg-config file. See docs/consuming.md for consumption details.

## License

You choose one of three:

- **Noncommercial use:** free, under PolyForm Noncommercial 1.0.0.
- **GPL and AGPL projects:** free, under the GNU Affero GPL version 3 or later.
- **Closed commercial use:** needs a paid commercial license.

[LICENSE.md](LICENSE.md) has a table that says which one fits your case. [FAQ.md](FAQ.md) explains the reasons in plain words, and [COMMERCIAL.md](COMMERCIAL.md) covers commercial licenses. Notes on scope and intent are in [LEGAL.md](LEGAL.md).
