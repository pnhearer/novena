# Consuming novena

Release archives contain the header, libraries, license files, `NOTICE`, a
CMake package, and a pkg-config file. No Rust installation is needed to use a
prebuilt archive.

The repository also contains a C host example in `examples/c-host`. It resolves
functions by name, records and submits a clear, presents a frame, writes census
and shapes reports, and checks `novena_last_error` after an invalid lookup.

## C

Compile with `-I/path/to/novena/include`, link with `-L/path/to/novena/lib
-lnovena`, and load the resulting shared library using the platform's normal
loader search path. The complete interface is in `include/novena.h`.
The C interface catches panics at every exported function boundary. A panic returns NOVENA_INTERNAL_ERROR. novena_last_error returns the message on the same thread until its next library call.

## C++ with CMake

Install or unpack an archive and point CMake at its prefix:

```cmake
find_package(novena CONFIG REQUIRED)
target_link_libraries(my_host PRIVATE novena::novena)
```

For an unpacked archive, use `-DCMAKE_PREFIX_PATH=/path/to/novena`.

## pkg-config

```sh
PKG_CONFIG_PATH=/path/to/novena/lib/pkgconfig pkg-config --cflags --libs novena
```

## C#

The native library can be imported directly:

```csharp
using System.Runtime.InteropServices;

internal static class Novena {
    [DllImport("novena", CallingConvention = CallingConvention.Cdecl)]
    internal static extern uint novena_host_interface_version();
}
```

On .NET 5 and later, `NativeLibrary.Load` and `NativeLibrary.GetExport` can
be used when the library path is selected at runtime. Keep the native library
and its architecture aligned with the process.

Rust unwinding stays enabled because every exported function catches panics at
the FFI boundary and reports `NOVENA_INTERNAL_ERROR`; `panic = "abort"` would
prevent that diagnostic and terminate the host process instead.

## Vulkan feature

Build the optional Vulkan backend with `cargo build --features vulkan`. The
backend clears color and depth images, uploads supported buffer-to-texture
copies, reads presented images back to the host, and applies the host's render
scale. It does not execute draws or Vulkan texture-to-texture copies.

The shader translation hook is a Rust API. Register a `ShaderTranslator` on an
`Instance`, then enable translation before shader setup. novena resolves shader
addresses through memory-pool ranges, bounds host reads to 64 KiB, and retains
translated words or translation errors. The hook receives the unknown shader
stage because the observed records do not establish a stage.
