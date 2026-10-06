# 0011: running past the first frame

- Date: 2026-10-05
- Author: Khargoosh
- Covers: `signal_event` in `api/queue.rs`, the SignalEvent and ReportCounter handlers in `api/recording.rs`, the `storage` field of `Object::Event`

## What was learned

In note 0010 the program stopped after its first frame. Its own log, read in a later run, said its game thread had timed out waiting for its render thread. The render thread was in a sleep loop that made no API calls, so it was polling memory. The candidates were the event storage the program had placed in pool memory (EventBuilderSetStorage) and the memory a counter report is written to (ReportCounter), both of which the graphics processor writes when it executes the recorded commands.

Handling those two commands at recording time, by writing the event value into the event's storage word and a non-zero report where a counter report was requested, let the program continue. In a 100-second run it then presented 3,226 frames and kept going until the run ended, with nothing drawn.

## How

Observation of an owned program in the host of note 0010, with the host's kernel call trace enabled to see what each thread was doing, and the program's own log messages. Novena's handlers were then changed and the run repeated.

## Confidence and open questions

- Which of the two memory words the render thread was polling is inferred from the fix working, not seen directly.
- The value written for a signalled event is the fourth argument register, which was 1 in every observed call. The layout of a counter report is a guess: sixteen bytes with a rising non-zero value in the second word.
- The program runs without any frame pacing, since nothing waits for a display. A real implementation will pace presents.
