# Clean-room rules

novena reimplements an interface. It must never contain, or be derived from, the proprietary implementation or its documentation. These rules exist so that the origin of every line can be shown.

## Sources that may be used

1. **Observation of software you lawfully own.** Which API functions a program asks for, the order and arguments of its calls, and the visible result, observed on your own copy with your own tools.
2. **Public facts.** Function and symbol names that programs carry in their own files, public specifications such as Vulkan and SPIR-V, published research, and openly licensed code whose license allows it to be included under every license this project is offered under. In practice that means permissive licenses such as MIT, BSD and Apache-2.0.
3. **Your own experiments.** Small test programs written for this project.
4. **Hardware behaviour documented in public.** Open drivers and public register documentation for the graphics processor, under their licenses.

## Sources that must not be used

The rule is general. It is not limited to one company or one product.

- **Any proprietary software development kit**, from any party: a platform holder, a hardware vendor, an engine or middleware vendor, or anyone else. That covers headers, libraries, samples, tools and documentation.
- **Anything under a non-disclosure agreement or other confidentiality terms**, whoever it came from and whatever it describes.
- **Leaked or otherwise improperly disclosed material of any kind.** A leak does not make material public for this purpose, and neither does finding it on a public website.
- **Proprietary source code**, including driver, firmware, operating system, engine and game source, however it was obtained.
- **Code taken from proprietary binaries.** Decompiled or disassembled code of any proprietary driver, library, firmware or program must not be copied, translated or closely paraphrased into this project. Reading a binary you own to learn what an interface does is observation. Carrying its code across is not.
- **Internal documents**: design documents, bug trackers, emails, slides, certification or guideline documents not published by their owner.

If you are unsure whether something falls under one of these, treat it as excluded and ask.

## Open code that may be read but not copied

Some openly published code has a license that does not allow it to be included here. Copyleft licenses such as the GPL family are the main case: novena is also offered under noncommercial and commercial terms, which other people's copyleft code cannot be placed under.

Such code is not excluded material. It is public, and reading it to understand a general technique is allowed where its license allows. What is not allowed is copying it, translating it, or writing something that follows it closely. If a part of novena was informed by reading such a project, say so in the provenance note.

## Separation of people

Anyone who has seen material from the "must not be used" list does not write, review or advise on the parts of novena that reimplement what that material covers. They are welcome in parts that do not touch it, such as the Vulkan backend internals, build tooling and tests of public behaviour.

If you are not sure which side of that line you are on, say so before contributing. Nobody will hold it against you. An undeclared problem is the only kind that can hurt the project.

## Provenance notes

Each function, structure layout, constant and behaviour in the library points to a note under `docs/provenance/`. A note records:

- what was learned,
- how it was learned (which kind of permitted source, which experiment),
- who did the work and when,
- what is still a guess.

A change that adds behaviour without a note is not merged. The format is in [docs/provenance/README.md](docs/provenance/README.md).

## What stays out of the repository

- Game files, dumps, shaders taken from games, captures of copyrighted output, keys and firmware.
- Traces that contain a program's data rather than the shape of its calls. A list of function names and call counts is fine. A buffer of a game's vertices or textures is not.

Tests use programs and assets written for this project.

## If something goes wrong

If material that breaks these rules is found in the repository, it is removed, the affected work is redone by someone who has not seen it, and the incident is written down in the provenance folder. Report it by opening an issue or, if you prefer, privately to a maintainer.
