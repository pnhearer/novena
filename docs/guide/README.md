# Guide

novena is a library for hosts that forward graphics calls. It keeps an object
table, records commands, and executes the supported operations on a CPU or
through an optional Vulkan backend. It exports Rust interfaces and a C interface
in [the public header](../../include/novena.h).

Start with [building and testing](building.md), then run the
[examples](examples.md). [Architecture](architecture.md) explains the execution
path. [Startup shader caching](startup-cache.md) explains registration, worker
requests, persistence, and diagnostics.

The function table contains 534 names. A known name does not imply a working
handler. Unsupported calls return `Status::Unimplemented`, count the call, and
zero the result registers. The stored census and signatures describe the
observed calls, not a promise of complete compatibility.

Evidence: [census](../census/README.md),
[signatures](../signatures/README.md), and
[command coverage](../provenance/0028-command-evidence.md).
