# Provenance notes

Every piece of API behaviour in novena is tied to a note in this folder. A note says what was learned and how, so the origin of the code can be shown.

## File names

`NNNN-short-title.md`, numbered in order of creation. Notes are not renumbered. A note that turns out wrong is corrected in place with a dated line that says what changed.

## Template

```
# NNNN: short title

- Date: YYYY-MM-DD
- Author: name or handle
- Covers: the functions, structures or constants this note supports

## What was learned

Plain statement of the behaviour.

## How

Which permitted source (see CLEAN-ROOM.md): observation of owned software,
public fact, own experiment, or public hardware documentation. Describe the
method well enough that someone else could repeat it. Name tools. Do not
include any program's data.

## Confidence and open questions

What is certain, what is inferred, what is a guess.
```

## What does not belong in a note

Game data, proprietary text, or anything copied from material the clean-room rules exclude. Describe the shape of what was seen (names, counts, order, sizes). Leave the content out.
