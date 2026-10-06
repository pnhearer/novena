# Consuming novena

Release archives contain the header, libraries, license files, `NOTICE`, a
CMake package and a pkg-config file. No Rust installation is needed to use a
prebuilt archive.

## C

Compile with `-I/path/to/novena/include`, link with `-L/path/to/novena/lib
-lnovena`, and load the resulting shared library using the platform's normal
loader search path. The complete interface is in `include/novena.h`.

## C++ with CMake

Install or unpack an archive and point CMake at its prefix:

```cmake
find_package(novena CONFIG REQUIRED)
target_link_libraries(my_host PRIVATE novena::novena)
```

For an unpacked archive, use `-DCMAKE_PREFIX_PATH=/path/to/novena`.

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
