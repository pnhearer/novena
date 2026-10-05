# Shape files

Each file is the output of `novena_instance_write_shapes` from one run of one program: for every function the program called, what its argument registers held in the first 2,048 calls and what its result registers held afterwards.

A shape file holds ranges, a few distinct small values, and counts. Addresses are counted and never printed. Values too wide for 32 bits are printed as `wide`. The memory behind an address is never read into the file, and the program is not named.

How to read a line:

- `address`, `constant-address`, `address-or-null`: every sampled value (or every non-zero one) was memory the host could read.
- `small`: every value was below 65,536.
- `constant`: one value in every sampled call. This is what an unused register usually looks like, because it still holds what the caller last put there.
- `number`, `mixed`: anything else.
- `f32=[...]` or `f64=[...]` appears when every distinct value reads as a plausible floating-point number.

| File | What ran | Provenance note |
|---|---|---|
| `0001-program-a-startup.txt` | Program A, from start-up for a little over three minutes, with no input | [0007](../provenance/0007-first-signatures.md) |
