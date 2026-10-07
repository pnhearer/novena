# Shader program data layout

Observed (2026-10-07) by dumping, for every `nvnProgramSetShaders` record in one real program's run, the memory at the address in the record's first word (novena's opt-in `NOVENA_RECORD_DUMP_DIR` observation). Facts, from 1,553 distinct records:

- The data starts with a 32-bit magic word.
- `0x12345678` (868 records): a graphics program. An 80-byte SM 5.x shader header is at +0x30 and code starts at +0x80. The code uses interpolation (IPA) or vertex attribute loads, matching the header's shader type.
- `0x12345679` (685 records): a compute program. +0x30..+0x100 is zero (no shader header) and code starts at +0x100. The code reads system registers (S2R), as compute code does.

novena now reads the magic and passes the translator the header followed by the code; for a compute program it passes an all-zero header, which the shader header format defines as the compute type. Any other magic is reported as an unknown layout.

Earlier, novena read memory only from +0x80, so the header it passed was always zero and every graphics program was treated as compute; and compute programs, whose code starts at +0x100, were reported as having no code. Both are fixed by this rule.
