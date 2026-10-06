//! An instance: what a host creates once and forwards a program's graphics
//! calls to.

use crate::api::{self, Handler, Objects};
use crate::functions::{self, FunctionId};
use crate::observe::{CallSnapshot, FunctionShape};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// What the host provides. The program's memory is the host's to manage, so
/// the library reads and writes it through these callbacks.
///
/// Both callbacks return 0 on success and any other value when the range is
/// not accessible. They are `unsafe` to call: the pointer must be valid for
/// `size` bytes.
///
/// The host's side of the contract, which `Instance::with_host` takes on
/// trust: `user` and both callbacks stay usable for the life of the
/// instance, and may be used from several threads at once, because a program
/// can call the graphics API from any of its threads.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Host {
    /// Passed back as the first argument of every callback.
    pub user: *mut c_void,
    pub read_memory: Option<
        unsafe extern "C" fn(user: *mut c_void, address: u64, out: *mut u8, size: u64) -> i32,
    >,
    pub write_memory: Option<
        unsafe extern "C" fn(user: *mut c_void, address: u64, data: *const u8, size: u64) -> i32,
    >,
}

// SAFETY: a `Host` only reaches an instance through `Instance::with_host`,
// whose caller guarantees that `user` and the callbacks may be shared between
// threads. The library itself only hands `user` back to the callbacks.
unsafe impl Send for Host {}
unsafe impl Sync for Host {}

/// The registers that carry a call's arguments and results under the
/// program's standard calling convention: eight integer registers, eight
/// floating-point registers (their low 64 bits) and the stack pointer, for
/// arguments that did not fit in registers.
///
/// The host fills this in before a call. On return `x[0]`, `x[1]` and `d[0]`
/// hold the results.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub x: [u64; 8],
    pub d: [u64; 8],
    pub sp: u64,
}

/// Outcome of a call into the library.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The function ran.
    Ok = 0,
    /// The function has no behaviour yet. The call was counted and the
    /// result registers were set to zero.
    Unimplemented = 1,
    /// The function id is outside the table.
    BadFunction = 2,
    /// A pointer argument was null, or a file could not be written.
    BadArgument = 3,
}

thread_local! {
    /// The call this thread last reported and what its address arguments
    /// pointed to, until the matching return is reported.
    static PENDING: Cell<Option<(u32, CallSnapshot)>> = const { Cell::new(None) };
}

pub struct Instance {
    host: Option<Host>,
    /// novena's record of the program's objects.
    pub objects: Objects,
    handlers: Vec<Option<Handler>>,
    requested: Vec<AtomicBool>,
    calls: Vec<AtomicU64>,
    shapes: Vec<Mutex<FunctionShape>>,
    /// Names the program asked for that are not in the table, with how often.
    unknown_requests: Mutex<BTreeMap<String, u64>>,
}

impl Instance {
    /// An instance with no host. It can record requests and calls, which is
    /// all the library does so far.
    pub fn new() -> Self {
        Self::build(None)
    }

    /// An instance that uses `host` to reach the program's memory.
    ///
    /// # Safety
    /// `host.user` and the callbacks must stay usable for the life of the
    /// instance and must tolerate being used from several threads at once.
    pub unsafe fn with_host(host: Host) -> Self {
        Self::build(Some(host))
    }

    fn build(host: Option<Host>) -> Self {
        let count = functions::count();
        Self {
            host,
            objects: Objects::new(),
            handlers: functions::all()
                .map(|(_, name)| api::handler(name))
                .collect(),
            requested: (0..count).map(|_| AtomicBool::new(false)).collect(),
            calls: (0..count).map(|_| AtomicU64::new(0)).collect(),
            shapes: (0..count).map(|_| Mutex::default()).collect(),
            unknown_requests: Mutex::new(BTreeMap::new()),
        }
    }

    /// The host this instance was created with.
    pub fn host(&self) -> Option<&Host> {
        self.host.as_ref()
    }

    /// Record that the program asked for `name` and return its id.
    pub fn request(&self, name: &str) -> Option<FunctionId> {
        match functions::lookup(name) {
            Some(id) => {
                self.requested[id.0 as usize].store(true, Ordering::Relaxed);
                Some(id)
            }
            None => {
                let mut unknown = self
                    .unknown_requests
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *unknown.entry(name.to_string()).or_default() += 1;
                None
            }
        }
    }

    /// Read program memory through the host. False when there is no host,
    /// no read callback, or the range is not accessible.
    pub fn read_memory(&self, address: u64, out: &mut [u8]) -> bool {
        let Some(read) = self.host.as_ref().and_then(|host| host.read_memory) else {
            return false;
        };
        let user = self
            .host
            .as_ref()
            .map_or(std::ptr::null_mut(), |host| host.user);
        // SAFETY: `out` is valid for its length; the host's callback reports
        // an inaccessible range instead of faulting.
        unsafe { read(user, address, out.as_mut_ptr(), out.len() as u64) == 0 }
    }

    /// Write program memory through the host.
    pub fn write_memory(&self, address: u64, data: &[u8]) -> bool {
        let Some(write) = self.host.as_ref().and_then(|host| host.write_memory) else {
            return false;
        };
        let user = self
            .host
            .as_ref()
            .map_or(std::ptr::null_mut(), |host| host.user);
        // SAFETY: `data` is valid for its length.
        unsafe { write(user, address, data.as_ptr(), data.len() as u64) == 0 }
    }

    /// Run a call. A function with a handler runs it; any other known
    /// function is counted and reported as unimplemented with zeroed
    /// results.
    pub fn call(&self, function: FunctionId, registers: &mut Registers) -> Status {
        let Some(counter) = self.calls.get(function.0 as usize) else {
            return Status::BadFunction;
        };
        // The first calls of each function are sampled for their shape; after
        // that only the counter is touched.
        let snapshot = if counter.fetch_add(1, Ordering::Relaxed) < crate::observe::SAMPLE_LIMIT {
            self.shape(function)
                .record_call(self.host.as_ref(), registers)
        } else {
            None
        };
        PENDING.set(snapshot.map(|snapshot| (function.0, snapshot)));
        if let Some(Some(handler)) = self.handlers.get(function.0 as usize) {
            return handler(self, function, registers);
        }
        registers.x[0] = 0;
        registers.x[1] = 0;
        registers.d[0] = 0;
        Status::Unimplemented
    }

    /// A host that lets the original implementation run reports what it
    /// returned here, so results are sampled along with arguments.
    pub fn returned(&self, function: FunctionId, registers: &Registers) -> Status {
        if function.0 as usize >= self.shapes.len() {
            return Status::BadFunction;
        }
        // The snapshot only belongs to this return if the thread's last
        // reported call was the same function.
        let snapshot = PENDING
            .take()
            .filter(|(pending, _)| *pending == function.0)
            .map(|(_, snapshot)| snapshot);
        self.shape(function)
            .record_return(self.host.as_ref(), registers, snapshot.as_ref());
        Status::Ok
    }

    fn shape(&self, function: FunctionId) -> std::sync::MutexGuard<'_, FunctionShape> {
        self.shapes[function.0 as usize]
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The sampled shapes of every function that was called, as text.
    pub fn shapes_report(&self) -> String {
        let mut out = String::from(
            "# novena shapes: what the argument and result registers held, per function\n",
        );
        for (id, name) in functions::all() {
            let calls = self.call_count(id);
            if calls > 0 {
                self.shape(id).write(&mut out, name, calls);
            }
        }
        out
    }

    pub fn call_count(&self, function: FunctionId) -> u64 {
        self.calls
            .get(function.0 as usize)
            .map_or(0, |counter| counter.load(Ordering::Relaxed))
    }

    /// A snapshot of what was requested and called.
    pub fn census(&self) -> Census {
        let functions = functions::all()
            .map(|(id, name)| CensusEntry {
                name,
                requested: self.requested[id.0 as usize].load(Ordering::Relaxed),
                calls: self.calls[id.0 as usize].load(Ordering::Relaxed),
            })
            .collect();
        let unknown_requests = self
            .unknown_requests
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        Census {
            functions,
            unknown_requests,
        }
    }
}

impl Default for Instance {
    fn default() -> Self {
        Self::new()
    }
}

struct CensusEntry {
    name: &'static str,
    requested: bool,
    calls: u64,
}

/// Which functions a program requested and how often it called each. It
/// holds names and counts only, never a program's data.
pub struct Census {
    functions: Vec<CensusEntry>,
    unknown_requests: BTreeMap<String, u64>,
}

impl Census {
    /// Number of distinct functions called at least once.
    pub fn functions_called(&self) -> usize {
        self.functions
            .iter()
            .filter(|entry| entry.calls > 0)
            .count()
    }

    /// Number of distinct known functions the program requested.
    pub fn functions_requested(&self) -> usize {
        self.functions
            .iter()
            .filter(|entry| entry.requested)
            .count()
    }
}

/// The text form: a summary line, then one line per function that was
/// requested or called, then the unknown names.
impl fmt::Display for Census {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "# novena census: {} of {} functions requested, {} called, {} unknown names requested",
            self.functions_requested(),
            self.functions.len(),
            self.functions_called(),
            self.unknown_requests.len()
        )?;
        writeln!(f, "# calls requested name")?;
        for entry in &self.functions {
            if entry.requested || entry.calls > 0 {
                let requested = if entry.requested { "yes" } else { "no" };
                writeln!(f, "{} {} {}", entry.calls, requested, entry.name)?;
            }
        }
        for (name, count) in &self.unknown_requests {
            writeln!(f, "unknown {count} {name}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_is_counted_and_reported_unimplemented() {
        let instance = Instance::new();
        let id = FunctionId(3);
        let mut registers = Registers {
            x: [1, 2, 3, 4, 5, 6, 7, 8],
            d: [9; 8],
            sp: 0x1000,
        };
        assert_eq!(instance.call(id, &mut registers), Status::Unimplemented);
        assert_eq!(instance.call(id, &mut registers), Status::Unimplemented);
        assert_eq!(instance.call_count(id), 2);
        assert_eq!((registers.x[0], registers.x[1], registers.d[0]), (0, 0, 0));
        // Arguments other than the result registers are left alone.
        assert_eq!(registers.x[2], 3);
        assert_eq!(registers.sp, 0x1000);
    }

    #[test]
    fn an_id_outside_the_table_is_refused() {
        let instance = Instance::new();
        let id = FunctionId(functions::count() as u32);
        assert_eq!(
            instance.call(id, &mut Registers::default()),
            Status::BadFunction
        );
        assert_eq!(instance.call_count(id), 0);
    }

    #[test]
    fn the_census_lists_requests_calls_and_unknown_names() {
        let instance = Instance::new();
        let (first, first_name) = functions::all().next().unwrap();
        let (second, second_name) = functions::all().nth(1).unwrap();
        assert_eq!(instance.request(first_name), Some(first));
        assert_eq!(instance.request("somethingElse"), None);
        assert_eq!(instance.request("somethingElse"), None);
        instance.call(second, &mut Registers::default());

        let census = instance.census();
        assert_eq!(census.functions_requested(), 1);
        assert_eq!(census.functions_called(), 1);

        let text = census.to_string();
        assert!(text.contains(&format!("0 yes {first_name}\n")), "{text}");
        assert!(text.contains(&format!("1 no {second_name}\n")), "{text}");
        assert!(text.contains("unknown 2 somethingElse\n"), "{text}");
        assert_eq!(text.lines().count(), 5, "{text}");
    }

    #[test]
    fn counting_is_safe_from_several_threads() {
        let instance = Instance::new();
        let id = FunctionId(0);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..1000 {
                        instance.call(id, &mut Registers::default());
                    }
                });
            }
        });
        assert_eq!(instance.call_count(id), 4000);
    }
}
