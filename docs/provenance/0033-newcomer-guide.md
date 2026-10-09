# 0033: newcomer guide and runnable examples

- Date: 2026-10-09
- Author: Khargoosh
- Covers: guide pages, public Rust documentation, and original examples

## Evidence boundary

The guide describes existing code. Guest call shapes and layouts retain the
evidence in signatures, shapes, census, and provenance notes. No new guest
signature, layout, enum mapping, or instruction encoding is introduced.
The architecture page links each execution path to its existing evidence.

The crate introduction previously said every call was unimplemented.
The handlers, command executor, and existing tests establish supported behavior.
The introduction now describes those paths and their explicit host contracts.
The crate denies missing public documentation in both feature configurations.

Public fields document retained raw values separately from host interpretations.
Memory offsets, dimensions, cache counters, and ownership follow their current
types and implementations. Startup documentation follows notes 0031 and 0032
and the existing worker and cache methods.

## Original examples

The cleared-window example follows the command recording and CPU clear evidence
in signatures 0002, note 0012, and note 0026. Its host callback copies a fixed
RGBA image and X11 displays that copy. A headless mode verifies every byte of
the same frame. It adds no native presentation behavior to the library.

The textured triangle follows the bounded host contracts and synthetic source
shader tests in notes 0027 through 0030. Vertices, checkerboard texels, and GLSL
are original. The local hook selects compiled SPIR-V using original marker
bytes. The observed envelope from note 0021 is used only to pass those markers
through registration; it is not a shader instruction fixture. The example
uses explicit synthetic format, topology, stage, and sampler tokens.

Rendering goes through the instance, command recording, registered pools,
texture and sampler bindings, asynchronous graphics cache, and canonical arena
readback. The example retries pending compilation with a bounded timeout.
A PPM image is the reviewable output. No recorded program data or external
translation implementation is included.

## Verification

Formatting, warnings-denied clippy, and warnings-denied Rust documentation
passed for the default and all-feature configurations. The default test suite
and complete all-feature test suite passed with exit code zero. The feature
run included ignored GPU and external translator checks, reported no skip
markers, and left no executable test ignored. Native ASTC images were explicitly
reported unavailable; their format geometry and byte conversion checks passed.

The C example compiled with warnings denied. Its headless mode verified every
cleared pixel. The triangle compiled and validated its generated modules,
verified rendered colors and geometry, and wrote the expected PPM image.
Native X11 display testing was unavailable because the audited account could
not authenticate to the display. The window event and X11 drawing path were
compiled but not exercised in that run.

Local guide links resolve. The publication check covered tracked files and the
new examples and documents. Existing Rust execution code is unchanged apart
from equivalent formatting of documented record fields and the missing-docs lint.
