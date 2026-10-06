# Provenance 0014: texture copies

This change uses observations in signatures 0002, 0003 and 0004. They show
the buffer GPU address and texture object register positions for the buffer
copy, but do not establish the later pointer records. Texture-to-texture
observations do not vary, so their geometry is unresolved.

Novena's own choice is to execute a complete base-level opaque byte copy for
the established source and destination pair. Buffer bytes are read through
the host callback in 64 KiB chunks because a GPU address is the program
address of pool storage, as recorded in provenance 0012. Unknown formats are
not decoded or swizzled. The unresolved pointer records and Vulkan format
mapping remain unimplemented and are retained as raw command information.
