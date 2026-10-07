# 0012: translated shader dumps

- Date: 2026-10-07
- Author: Khargoosh
- Covers: the optional translated-shader dump directory

## What was learned

A host can select a directory for off-by-default debugging output. Each
successful translation produces a SPIR-V file named from the program object
address and shader record index. Each translation failure produces a one-line
error file. The original shader bytes are not written.

## How

Own experiment using a fake `ShaderTranslator` and a host memory callback.
The experiment enabled translation, selected a temporary directory, submitted
one retained shader record, and checked the resulting translated words and
error text. The file format is the public SPIR-V word representation produced
by the translator hook.

## Confidence and open questions

The file naming and contents are implementation-defined debugging output. File
creation errors are ignored so debugging output cannot change translation
results.
