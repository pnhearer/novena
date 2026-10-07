# 0017: optional shader translation

- Date: 2026-10-07
- Author: Khargoosh
- Covers: optional translation during `ProgramSetShaders`, retained program results, and census errors

## What was learned

When the translation option is enabled and a translator is registered, each
retained shader record is resolved through the registered memory-pool ranges.
The first GPU-shaped record value is used as the code location, and host reads
are bounded to 64 KiB. A zero run found by the observation length scan limits
the bytes passed to the translator. The translator receives the explicit
unknown stage value because the records do not establish a stage. Successful
SPIR-V words and translation error strings are retained per program object.
Census output counts each error message and never includes shader bytes.

## How

This is an own implementation experiment based only on the record and pool
shapes in signatures 0007 and 0008 and the resolver in provenance 0016. Tests
use a fake host memory callback and a fake translator created for novena.

## Confidence and open questions

The option, byte bound, result retention, and error census are implementation
behavior. The choice of the first GPU-shaped value as code and the unknown
stage are explicit assumptions required by the still-open observations.
