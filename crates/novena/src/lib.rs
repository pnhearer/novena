//! novena: a Vulkan implementation of a console graphics API.
//!
//! The library has no API-facing functions yet. What exists is the part of
//! the C interface that describes the library itself, so hosts can already
//! link it and check what they loaded. See `docs/design.md` for the plan and
//! `CLEAN-ROOM.md` for the rules every addition follows.

use std::ffi::c_char;

/// Version of the host interface. It changes when a host built against an
/// older `include/novena.h` could no longer use the library.
pub const HOST_INTERFACE_VERSION: u32 = 0;

static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

/// The library version as a NUL-terminated string that lives as long as the
/// library is loaded.
#[no_mangle]
pub extern "C" fn novena_version() -> *const c_char {
    VERSION.as_ptr().cast()
}

/// The host interface version this library was built with.
#[no_mangle]
pub extern "C" fn novena_host_interface_version() -> u32 {
    HOST_INTERFACE_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn version_is_a_c_string_matching_the_package() {
        // SAFETY: `novena_version` returns a pointer to a static
        // NUL-terminated string.
        let version = unsafe { CStr::from_ptr(novena_version()) };
        assert_eq!(version.to_str(), Ok(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn host_interface_version_matches_the_constant() {
        assert_eq!(novena_host_interface_version(), HOST_INTERFACE_VERSION);
    }
}
