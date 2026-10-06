# 0013: Vulkan backend

- Date: 2026-10-06
- Author: Khargoosh
- Covers: optional Vulkan image execution and host presentation

## What was built

novena can allocate RGBA8 images for window-presented textures, clear them on a
graphics queue, copy them through a host-visible staging buffer, and pass the
scaled result to the host. Depth clears use a D32 floating-point image. The
CPU implementation remains the fallback.

The image format choices, scale rounding, one-shot command submission, and
device selection are novena's own choices. No new target API behavior was
observed for this backend.

## How

Public Vulkan and ash documentation, plus tests written for novena. No
proprietary SDK, leaked material, or target implementation was used.

## Confidence and open questions

Clear and readback behavior is covered by Vulkan tests when a device is
available. Draw execution, texture format mapping, and persistent layout
tracking remain future work.
