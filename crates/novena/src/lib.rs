//! novena: a Vulkan implementation of a console graphics API.
//!
//! What exists so far is the outer shell every later piece hangs on:
//!
//! - the table of function names a program may request ([`functions`]),
//! - an instance that a host creates and forwards the program's calls to,
//! - a census of which functions were requested and called.
//!
//! No function has behaviour yet. A call is counted, its result registers are
//! zeroed, and the caller is told it was not implemented. See
//! `docs/design.md` for the plan and `CLEAN-ROOM.md` for the rules every
//! addition follows.

pub mod api;
pub mod functions;
#[cfg(feature = "vulkan")]
pub mod gpu;
mod instance;
pub mod observe;

pub use instance::{Census, Host, Instance, Registers, Status};

use functions::FunctionId;
use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Version of the host interface. It changes when a host built against an
/// older `include/novena.h` could no longer use the library.
pub const HOST_INTERFACE_VERSION: u32 = 4;

/// Returned by lookups for a name the library does not know.
pub const FUNCTION_NONE: u32 = u32::MAX;

static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

fn ffi_guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    LAST_ERROR.with(|error| *error.borrow_mut() = None);
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            LAST_ERROR.with(|error| {
                *error.borrow_mut() = Some(
                    CString::new(message)
                        .unwrap_or_else(|_| CString::new("panic message contained NUL").unwrap()),
                )
            });
            default
        }
    }
}

/// The most recent panic message on this thread, or null when there was none.
#[no_mangle]
pub extern "C" fn novena_last_error() -> *const c_char {
    // Not wrapped in `ffi_guard`, which would clear the message first.
    LAST_ERROR.with(|error| {
        error
            .borrow()
            .as_ref()
            .map_or(std::ptr::null(), |message| message.as_ptr())
    })
}

/// The library version as a NUL-terminated string that lives as long as the
/// library is loaded.
#[no_mangle]
pub extern "C" fn novena_version() -> *const c_char {
    ffi_guard(std::ptr::null(), || VERSION.as_ptr().cast())
}

/// The host interface version this library was built with.
#[no_mangle]
pub extern "C" fn novena_host_interface_version() -> u32 {
    ffi_guard(0, || HOST_INTERFACE_VERSION)
}

/// Number of functions in the table. Valid function ids are `0..count`.
#[no_mangle]
pub extern "C" fn novena_function_count() -> u32 {
    ffi_guard(0, || functions::count() as u32)
}

/// Name of a function as a NUL-terminated string owned by the library, or
/// null for an id outside the table.
#[no_mangle]
pub extern "C" fn novena_function_name(function: u32) -> *const c_char {
    ffi_guard(std::ptr::null(), || {
        functions::c_name(FunctionId(function)).map_or(std::ptr::null(), CStr::as_ptr)
    })
}

/// Id of the function with this name, or `FUNCTION_NONE`.
///
/// # Safety
/// `name` must be null or point to a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn novena_function_lookup(name: *const c_char) -> u32 {
    ffi_guard(FUNCTION_NONE, || {
        if name.is_null() {
            return FUNCTION_NONE;
        }
        // SAFETY: the caller passes a NUL-terminated string.
        let name = unsafe { CStr::from_ptr(name) };
        name.to_str()
            .ok()
            .and_then(functions::lookup)
            .map_or(FUNCTION_NONE, |id| id.0)
    })
}

/// Create an instance. `host` may be null for a host that provides nothing
/// yet; the structure is copied. Destroy the result with
/// `novena_instance_destroy`.
///
/// # Safety
/// `host` must be null or point to a valid `Host`. Its `user` pointer and
/// callbacks must stay usable for the life of the instance and must tolerate
/// being used from several threads at once.
#[no_mangle]
pub unsafe extern "C" fn novena_instance_create(host: *const Host) -> *mut Instance {
    ffi_guard(std::ptr::null_mut(), || {
        // SAFETY: the caller passes null or a valid `Host`.
        let instance = match unsafe { host.as_ref() } {
            Some(host) => unsafe { Instance::with_host(*host) },
            None => Instance::new(),
        };
        Box::into_raw(Box::new(instance))
    })
}

/// Destroy an instance. Null is accepted.
///
/// # Safety
/// `instance` must be null or come from `novena_instance_create` and not
/// have been destroyed already. No other call on the instance may be running
/// or start afterwards.
#[no_mangle]
pub unsafe extern "C" fn novena_instance_destroy(instance: *mut Instance) {
    ffi_guard((), || {
        if !instance.is_null() {
            drop(unsafe { Box::from_raw(instance) });
        }
    })
}

/// The program asked its bootstrap function for `name`. Records the request
/// and returns the function id, or `FUNCTION_NONE` for a name the library
/// does not know (which is recorded too).
///
/// # Safety
/// `instance` must be live; `name` must be null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn novena_instance_request(
    instance: *const Instance,
    name: *const c_char,
) -> u32 {
    ffi_guard(FUNCTION_NONE, || {
        let Some(instance) = (unsafe { instance.as_ref() }) else {
            return FUNCTION_NONE;
        };
        if name.is_null() {
            return FUNCTION_NONE;
        }
        let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
        instance.request(&name).map_or(FUNCTION_NONE, |id| id.0)
    })
}

/// The program called `function`. `registers` holds its argument registers
/// on entry and receives its result registers.
///
/// # Safety
/// `instance` must be live. `registers` must point to a valid `Registers`
/// that nothing else reads or writes until the call returns. Calls on one
/// instance may run on several threads at once, each with its own registers.
#[no_mangle]
pub unsafe extern "C" fn novena_instance_call(
    instance: *const Instance,
    function: u32,
    registers: *mut Registers,
) -> Status {
    ffi_guard(Status::InternalError, || {
        match unsafe { (instance.as_ref(), registers.as_mut()) } {
            (Some(instance), Some(registers)) => instance.call(FunctionId(function), registers),
            _ => Status::BadArgument,
        }
    })
}

/// The original implementation of `function` returned. `registers` holds
/// its result registers. Only hosts that let the original implementation run
/// call this; it lets results be sampled along with arguments.
///
/// # Safety
/// `instance` must be live and `registers` must point to a valid `Registers`.
#[no_mangle]
pub unsafe extern "C" fn novena_instance_returned(
    instance: *const Instance,
    function: u32,
    registers: *const Registers,
) -> Status {
    ffi_guard(Status::InternalError, || {
        match unsafe { (instance.as_ref(), registers.as_ref()) } {
            (Some(instance), Some(registers)) => instance.returned(FunctionId(function), registers),
            _ => Status::BadArgument,
        }
    })
}

/// Write the sampled argument and result shapes to a text file.
///
/// # Safety
/// `instance` must be live; `path` must be NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn novena_instance_write_shapes(
    instance: *const Instance,
    path: *const c_char,
) -> Status {
    ffi_guard(Status::InternalError, || {
        // SAFETY: the caller passes a live instance.
        let Some(instance) = (unsafe { instance.as_ref() }) else {
            return Status::BadArgument;
        };
        if path.is_null() {
            return Status::BadArgument;
        }
        // SAFETY: the caller passes a NUL-terminated string.
        let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
        match std::fs::write(&*path, instance.shapes_report()) {
            Ok(()) => Status::Ok,
            Err(_) => Status::BadArgument,
        }
    })
}

/// How many times `function` was called on this instance.
///
/// # Safety
/// `instance` must be null or live.
#[no_mangle]
pub unsafe extern "C" fn novena_instance_call_count(
    instance: *const Instance,
    function: u32,
) -> u64 {
    ffi_guard(0, || {
        unsafe { instance.as_ref() }.map_or(0, |instance| instance.call_count(FunctionId(function)))
    })
}

/// Write the census (what was requested and called) to a text file.
/// Returns `Status::Ok`, or `Status::BadArgument` when the file could not be
/// written.
///
/// # Safety
/// `instance` must be live; `path` must be NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn novena_instance_write_census(
    instance: *const Instance,
    path: *const c_char,
) -> Status {
    ffi_guard(Status::InternalError, || {
        // SAFETY: the caller passes a live instance.
        let Some(instance) = (unsafe { instance.as_ref() }) else {
            return Status::BadArgument;
        };
        if path.is_null() {
            return Status::BadArgument;
        }
        // SAFETY: the caller passes a NUL-terminated string.
        let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
        match std::fs::write(&*path, instance.census().to_string()) {
            Ok(()) => Status::Ok,
            Err(_) => Status::BadArgument,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn version_is_a_c_string_matching_the_package() {
        // SAFETY: `novena_version` returns a static NUL-terminated string.
        let version = unsafe { CStr::from_ptr(novena_version()) };
        assert_eq!(version.to_str(), Ok(env!("CARGO_PKG_VERSION")));
        assert_eq!(novena_host_interface_version(), HOST_INTERFACE_VERSION);
    }

    #[test]
    fn a_panic_is_reported_and_cleared_by_the_next_call() {
        let status = ffi_guard(Status::Ok, || -> Status { panic!("boom") });
        assert_eq!(status, Status::Ok);
        // SAFETY: a non-null result is a NUL-terminated string owned by the
        // library until the next call on this thread.
        let message = unsafe { CStr::from_ptr(novena_last_error()) };
        assert_eq!(message.to_str(), Ok("boom"));
        novena_version();
        assert!(novena_last_error().is_null());
    }

    #[test]
    fn c_interface_round_trips_names_and_counts_calls() {
        let count = novena_function_count();
        assert!(count > 0);
        // SAFETY: ids below the count have names; the strings are static.
        let first = unsafe { CStr::from_ptr(novena_function_name(0)) }.to_owned();
        assert_eq!(unsafe { novena_function_lookup(first.as_ptr()) }, 0);
        assert!(novena_function_name(count).is_null());

        let unknown = CString::new("notAFunction").unwrap();
        assert_eq!(
            unsafe { novena_function_lookup(unknown.as_ptr()) },
            FUNCTION_NONE
        );

        // SAFETY: a null host is allowed, and the instance is destroyed below.
        let instance = unsafe { novena_instance_create(std::ptr::null()) };
        assert_eq!(
            unsafe { novena_instance_request(instance, first.as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { novena_instance_request(instance, unknown.as_ptr()) },
            FUNCTION_NONE
        );

        let mut registers = Registers::default();
        registers.x[0] = 7;
        let status = unsafe { novena_instance_call(instance, 0, &mut registers) };
        assert_eq!(status, Status::Unimplemented);
        assert_eq!(registers.x[0], 0);
        assert_eq!(unsafe { novena_instance_call_count(instance, 0) }, 1);
        assert_eq!(
            unsafe { novena_instance_call(instance, count, &mut registers) },
            Status::BadFunction
        );

        unsafe { novena_instance_destroy(instance) };
    }
}
