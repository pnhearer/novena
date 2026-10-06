# Signatures 0005: device integer query

Source: [shapes 0003](../shapes/0003-program-a-startup-answers.txt). Method and limits: [provenance note 0009](../provenance/0009-query-answers.md).

`DeviceGetInteger(device: object, selector: int, out: pointer)` writes one 32-bit integer through `out`. The program asked these selectors, and the platform's own implementation answered:

| Selector | Answer |
|---|---|
| 0x00 | 0x37 (55) |
| 0x01 | 0xf (15) |
| 0x0f | 0x20 (32) |
| 0x10 | 0x20 (32) |
| 0x11 | 0x100 (256) |
| 0x12 | 0x100 (256) |
| 0x18 | 0x100 (256) |
| 0x1a | 0x20000 (131072) |
| 0x1f | 0x600 (1536) |
| 0x24 | 0x100000 (1048576) |
| 0x25 | 0x1000 (4096) |
| 0x2d | 0x800 (2048) |
| 0x3d | 0x4000 (16384) |
| 0x40 | 0x4000 (16384) |
| 0x46 | 0x5 |
| 0x56 | 0x1000 (4096) |
| 0x5c | 0x4000 (16384) |
| 0x5d | 0x1000 (4096) |

The same data is in `data/device-integers.txt` for the implementation to use. The meaning of each selector is unknown; the names in the function table do not cover them.
