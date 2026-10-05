# 0003: the function names

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `data/function-names.txt`, the function table in `crates/novena/src/functions.rs`

## What was learned

534 strings in one program's executable that have the form of the graphics API's function names. They are listed, sorted, in `data/function-names.txt`. We take them to be the names of functions the program requests, which is an inference (see below).

## How

Observation of an owned program. The program's own main executable was scanned for NUL-terminated text of the form: the API's three-letter lowercase prefix, then a capital letter, then letters and digits. That found 535 names. One of them is the bootstrap function itself, which the program imports instead of requesting, so it is left out. The other 534 are the list.

Only the program's own executable was read. The platform's system libraries and the graphics driver were not used as a source for this list, and no development kit material of any kind was consulted.

The scan can be repeated on any program by anyone who owns one: extract the executable's segments and search them with the pattern above.

## Confidence and open questions

- That these 534 strings are present in the program is certain.
- That every one of them is a function the program requests through the bootstrap function is inferred from their form and from note 0002. A string that merely looks like a function name would also have matched. The census from a real run will confirm or correct this.
- Whether other programs request names that are not in this list is unknown. The library records unknown names it is asked for, so a run of another program will show any.
- Nothing is known yet about what any function takes or returns. The names suggest groupings by object (the text after the prefix), and that is all.
