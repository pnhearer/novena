# 0016: GPU address resolution for pool storage

2026-10-07: [Provenance 0022](0022-flat-global-memory.md) supersedes the
program-storage GPU base choice when Vulkan is active. CPU-only behavior
retains the mapping described below.

- Date: 2026-10-07
- Author: Khargoosh
- Covers: memory pool GPU addresses and the `ProgramSetShaders` observation

## What was learned

The maintainer's shape-level observation reports that both GPU-shaped values
in every observed `ProgramSetShaders` call were unreadable through the host
callback. They are therefore not program addresses. A GPU-shaped value must
be resolved against the memory pool whose storage contains it before novena
asks the host to read the pointed-to shape.

Novena assigns each registered memory pool a GPU range beginning at the pool's
program storage address and extending for the size supplied to
`MemoryPoolBuilderSetStorage`. `MemoryPoolGetBufferAddress` returns that same
base. This is novena's implementation choice, consistent with the earlier
recorded texture and copy behavior. Resolution returns the pool and relative
offset, then the program address of the storage plus that offset.

## How

The pool storage pointer and size were read from
`docs/signatures/0003-objects.md` and the existing pool handler. The two
shader fields and the shape-only bounded read were read from
`docs/signatures/0007-program-shaders.md` and
`docs/signatures/0008-program-shader-state.md`. The resolver and fake-host
tests are an own implementation experiment; no external API layout or value
was used.

## Confidence and open questions

The pool registration fields, the returned pool base, and the need to resolve
the two shader GPU-shaped fields are established for this implementation.
The real address-space assignment remains unobserved, so novena's choice of
program storage as the GPU base is an implementation assumption. An address
outside every registered range is reported as unresolved; an address at or
past a matched pool's size is reported as outside that pool.
