# 0026: window and offscreen presentation

- Date: 2026-10-07
- Author: Khargoosh
- Covers: version-5 host additions, arena-backed render targets, Vulkan clears,
  image transfers, swapchain presentation, frame fences, and synthetic tests

## What was learned

The window builder carries a native window token and a list of texture object
addresses. Presentation selects a texture by index. Render-target and clear
arguments follow the existing signature records. No new guest argument,
structure layout, or format enum was inferred.

Vulkan surface creation depends on platform extensions enabled when the
instance is created. Surface support is queried for each available queue
family. Swapchain format, extent, image count, usage, transform, alpha mode,
and presentation mode must follow the surface capabilities.

A Vulkan blit performs format conversion and scaling. The colour components
retain their meanings when converting RGBA to BGRA. An sRGB destination
encodes the source's linear colour components. The backend checks blit support
for both formats before recording the command.

Transfer clears require transfer-destination usage and a valid image layout.
They clear whole colour texels and cannot implement a channel mask directly.
Novena uses a read, merge, and upload for partial masks. Full colour clears use
`vkCmdClearColorImage`. Depth clears use `vkCmdClearDepthStencilImage` on
the recorded depth target. A zero depth-write flag suppresses the depth clear.
Stencil clears remain unsupported.

A submission fence permits reuse of that submission's command buffer and
acquire semaphore. The fence does not prove that presentation consumed its
wait semaphore. Presentation wait semaphores therefore belong to swapchain
images and are reused only after reacquisition.

## How

Existing guest facts come from these project records:

- [Objects](../signatures/0003-objects.md)
- [Pointers](../signatures/0004-pointers.md)
- [Command buffers](../signatures/0002-command-buffer.md)
- [Copies and their unresolved arguments](../signatures/0006-copies.md)
- [CPU clear storage choices](0012-cpu-clears.md)
- [Flat arena and address mapping](0022-flat-global-memory.md)

New backend facts come from public Vulkan documentation:

- [Swapchain creation](https://docs.vulkan.org/refpages/latest/refpages/source/VkSwapchainCreateInfoKHR.html)
- [Xlib surface creation](https://docs.vulkan.org/refpages/latest/refpages/source/vkCreateXlibSurfaceKHR.html)
- [Image blits](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdBlitImage.html)
- [Colour clears](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdClearColorImage.html)
- [Buffer to image copies](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdCopyBufferToImage.html)
- [Image to buffer copies](https://docs.vulkan.org/refpages/latest/refpages/source/vkCmdCopyImageToBuffer.html)
- [Image layout barriers](https://docs.vulkan.org/refpages/latest/refpages/source/VkImageMemoryBarrier.html)
- [Subresource layouts](https://docs.vulkan.org/refpages/latest/refpages/source/vkGetImageSubresourceLayout.html)
- [Fence waits](https://docs.vulkan.org/refpages/latest/refpages/source/vkWaitForFences.html)
- [Swapchain acquisition](https://docs.vulkan.org/refpages/latest/refpages/source/vkAcquireNextImageKHR.html)
- [Presentation semaphore reuse](https://docs.vulkan.org/guide/latest/swapchain_semaphore_reuse.html)

The code and test data were written for novena. No external implementation
code, proprietary documentation, or program data was used.

## Implementation choices

Version 5 appends an optional pointer to a host Vulkan contract. The host
lists its platform instance extensions and creates a surface from the window
object and native token. Novena owns the returned surface. This avoids assuming
that a guest token is a host pointer or a particular platform window handle.

Canonical texture bytes occupy the texture's pool and offset in the flat
arena. Optimal Vulkan images provide the render representation. Explicit
buffer-image transfers load those bytes before operations and write them back
after clears and copies. Aliased texture objects reload the same arena bytes.
Buffer-to-texture copies read the Vulkan arena buffer directly, so they observe
clears earlier in the same recording before host bytes are downloaded.
The images do not alias the arena buffer's device allocation. Optimal image
tiling has no portable linear byte layout, so sharing an allocation alone
would not make shader byte accesses agree with image texels.

The existing four-byte base-level storage choice remains RGBA8 UNORM for
colour and now D32 floating point for depth. Dimensions remain unscaled in
the arena. Vulkan callback output applies the host render scale through a
blit. Native output uses the host drawable extent. No guest format integer
is mapped to a Vulkan format.

The presentation path prefers BGRA8 UNORM, then RGBA8 UNORM, then their sRGB
forms, in the surface's nonlinear sRGB colour space. Positive intervals use
FIFO. Zero requests immediate mode, with FIFO as fallback. Values above one
currently have FIFO's pacing rather than a counted number of refreshes.

Two command slots each own a pool, command buffer, fence, and acquire
semaphore. Layouts persist between submissions, with explicit access barriers.
Ordinary presentation waits on the reused slot's fence. Resizing, changing
presentation mode, out-of-date acquisition, and suboptimal presentation
rebuild the swapchain. A zero drawable extent skips presentation.
Retirement and destruction drain the device. As documented by Vulkan's
semaphore guide, this is the conventional unextended swapchain teardown;
a future maintenance extension could add presentation fences.

Submission still mirrors live pool bytes through host callbacks and waits
for arena transfers before downloading writes. That synchronization can limit
CPU and GPU overlap. Full colour clears and native presentation use device
commands and never read the presented pixels back to the CPU.

## Confidence and open questions

The headless tests exercise the actual API recording, submission, clear,
copy, and present callback. They compare every pixel and canonical arena
byte, including aliases and neighbouring bytes. The same-recording clear
then buffer-copy case failed with stale host bytes before the copy path changed
to read the arena buffer, and passed after the fix. A separate test shares the
window blit's implementation and checks BGRA order, sRGB encoding, scaling,
format changes, and repeated submissions.

The manual X11 example uses three arena-backed textures, cycles clear
colours, and handles resize and close. Its finite modes support CI smoke tests.

Only tightly packed, first-layer, base-level images are executed. Guest
formats, block-linear layouts, subrectangles, other mip levels, multisampling,
scissor semantics for clears, and stencil formats remain open. Draw execution
is still absent. Unsupported image shapes fail instead of inventing a layout.

## Verification

Run the repeatable pixel check without a display:

```sh
python3 scripts/check-presentation.py
```

Add `--window` with an X11 display to build and run the native example for
120 FIFO frames and 120 immediate frames, with two resizes in each run.
The script fails on nonzero process exit codes or Vulkan validation errors.
CI supplies a software Vulkan driver, validation layers, and a virtual display.
Full output goes to `target/presentation.log`.

On 2026-10-07, formatting and strict clippy passed with default features and
all features. Both locked workspace test configurations passed. The existing
explicit Vulkan pool-address and two global-memory shader tests passed.
The new pixel tests passed on a hardware driver and on a software driver
with Vulkan validation enabled. The native example passed 120 FIFO frames
and 120 immediate frames, with two resizes per run, on a virtual X11 display
and the software driver. No Vulkan validation errors were reported.
The legacy C host produced the expected black 4 by 3 PPM with Vulkan and
with CPU fallback. All task-owned virtual display processes exited.
