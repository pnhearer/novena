//! An instance: what a host creates once and forwards a program's graphics
//! calls to.

use crate::api::{self, Handler, Objects};
use crate::functions::{self, FunctionId};
#[cfg(feature = "vulkan")]
use crate::gpu::Backend;
use crate::observe::{CallSnapshot, FunctionShape};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::{c_char, c_void};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type ShaderStage = u32;
pub const SHADER_STAGE_UNKNOWN: ShaderStage = 0;

#[derive(Clone, Copy)]
pub(crate) struct PendingShader {
    pub record_index: usize,
    pub address: u64,
    pub limit: usize,
    pub failures: u8,
}

pub trait ShaderTranslator: Send + Sync {
    fn translate(&self, stage: ShaderStage, code: &[u8]) -> Result<Vec<u32>, String>;
}

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
/// can call the graphics API from any of its threads. A non-null `vulkan`
/// pointer follows the lifetime and callback contract in docs/host-interface.md.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Host {
    /// Passed back as the first argument of every callback.
    pub user: *mut c_void,
    pub read_memory: Option<
        unsafe extern "C" fn(user: *mut c_void, address: u64, out: *mut u8, size: u64) -> i32,
    >,
    pub write_memory: Option<
        unsafe extern "C" fn(user: *mut c_void, address: u64, data: *const u8, size: u64) -> i32,
    >,
    pub present: Option<
        unsafe extern "C" fn(
            user: *mut c_void,
            window_object: u64,
            width: u32,
            height: u32,
            rgba: *const u8,
            stride_bytes: u64,
        ),
    >,
    /// The output scale. Values below 1 are treated as 1.0.
    pub render_scale: f32,
    /// Called immediately before a texture is presented.
    pub wait_vblank: Option<unsafe extern "C" fn(user: *mut c_void)>,
    /// Optional version-5 Vulkan presentation contract. See docs/host-interface.md.
    pub vulkan: *const HostVulkan,
}

/// Host-owned platform integration. Vulkan handles cross this C boundary as u64.
/// Provenance: 0026-presentation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostVulkan {
    pub extension_count: u32,
    pub extensions: *const *const c_char,
    pub create_surface: unsafe extern "C" fn(
        user: *mut c_void,
        instance: u64,
        window_object: u64,
        native_window: u64,
    ) -> u64,
    pub drawable_size: unsafe extern "C" fn(
        user: *mut c_void,
        window_object: u64,
        width: *mut u32,
        height: *mut u32,
    ),
}

// SAFETY: a `Host` only reaches an instance through `Instance::with_host`,
// whose caller guarantees that `user` and the callbacks may be shared between
// threads. Native integration pointers follow the same documented contract.
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
    /// The library panicked while servicing the call.
    InternalError = 4,
}

thread_local! {
    /// The call this thread last reported and what its address arguments
    /// pointed to, until the matching return is reported.
    static PENDING: Cell<Option<(u32, CallSnapshot, Registers)>> = const { Cell::new(None) };
}

pub struct Instance {
    host: Option<Host>,
    /// novena's record of the program's objects.
    pub objects: Objects,
    handlers: Vec<Option<Handler>>,
    /// A rising count that stands in for the graphics processor's clock in
    /// counter reports.
    counter_reports: AtomicU64,
    requested: Vec<AtomicBool>,
    calls: Vec<AtomicU64>,
    shapes: Vec<Mutex<FunctionShape>>,
    /// Names the program asked for that are not in the table, with how often.
    unknown_requests: Mutex<BTreeMap<String, u64>>,
    shader_translator: Mutex<Option<Arc<dyn ShaderTranslator>>>,
    shader_translation_enabled: AtomicBool,
    shader_dump_directory: Mutex<Option<PathBuf>>,
    shader_dump_sequence: AtomicU64,
    shader_dump_hashes: Mutex<BTreeSet<u64>>,
    successful_shader_translations: AtomicU64,
    distinct_shader_outputs: Mutex<BTreeSet<u64>>,
    shader_translation_errors: Mutex<BTreeMap<String, u64>>,
    pending_shaders: Mutex<HashMap<u64, Vec<PendingShader>>>,
    shader_late_reads: Mutex<BTreeMap<&'static str, u64>>,
    shader_zero_header: AtomicU64,
    shader_header_without_code: AtomicU64,
    #[cfg(feature = "vulkan")]
    pub(crate) gpu: Mutex<Option<Backend>>,
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
    /// A non-null `host.vulkan` points to a valid HostVulkan for that lifetime.
    /// Its extension array and strings are valid during this call. Native windows
    /// stay live until finalization, and its callbacks must not reenter novena.
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
            counter_reports: AtomicU64::new(1),
            requested: (0..count).map(|_| AtomicBool::new(false)).collect(),
            calls: (0..count).map(|_| AtomicU64::new(0)).collect(),
            shapes: (0..count).map(|_| Mutex::default()).collect(),
            unknown_requests: Mutex::new(BTreeMap::new()),
            shader_translator: Mutex::new(None),
            shader_translation_enabled: AtomicBool::new(false),
            shader_dump_directory: Mutex::new(None),
            shader_dump_sequence: AtomicU64::new(0),
            shader_dump_hashes: Mutex::new(BTreeSet::new()),
            successful_shader_translations: AtomicU64::new(0),
            distinct_shader_outputs: Mutex::new(BTreeSet::new()),
            shader_translation_errors: Mutex::new(BTreeMap::new()),
            pending_shaders: Mutex::new(HashMap::new()),
            shader_late_reads: Mutex::new(BTreeMap::new()),
            shader_zero_header: AtomicU64::new(0),
            shader_header_without_code: AtomicU64::new(0),
            #[cfg(feature = "vulkan")]
            gpu: Mutex::new(host.and_then(|h| Backend::with_host(&h))),
        }
    }

    /// The host this instance was created with.
    pub fn host(&self) -> Option<&Host> {
        self.host.as_ref()
    }

    /// Enable the bounded first draw experiment with explicit host enum choices.
    /// Returns false when this instance has no Vulkan backend.
    #[cfg(feature = "vulkan")]
    pub fn set_first_draw_contract(
        &self,
        contract: Option<crate::gpu::graphics::FirstDrawContract>,
    ) -> bool {
        let mut gpu = self.gpu.lock().unwrap_or_else(|p| p.into_inner());
        let Some(backend) = gpu.as_mut() else {
            return false;
        };
        backend.first_draw = contract;
        true
    }

    #[cfg(feature = "vulkan")]
    pub fn graphics_cache_stats(&self) -> Option<crate::gpu::pipelines::CacheStats> {
        self.gpu
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|backend| backend.graphics.stats())
    }

    /// Configure private persistent graphics cache files and a bounded worker pool.
    /// Call outside queue submission. Reconfiguration drains the previous workers.
    #[cfg(feature = "vulkan")]
    pub fn set_graphics_pipeline_cache(
        &self,
        directory: &std::path::Path,
        identity: &crate::gpu::pipelines::TranslationIdentity,
        worker_count: usize,
        queue_capacity: usize,
    ) -> Result<(), String> {
        let mut gpu = self.gpu.lock().unwrap_or_else(|p| p.into_inner());
        let backend = gpu.as_mut().ok_or("Vulkan backend is unavailable")?;
        backend.graphics = crate::gpu::graphics::GraphicsPipelines::persistent(
            backend.context(),
            directory,
            identity,
            worker_count,
            queue_capacity,
        )?;
        Ok(())
    }

    /// Queued and compiling graphics requests. Polling performs no disk I/O.
    #[cfg(feature = "vulkan")]
    pub fn graphics_pending_count(&self) -> Option<usize> {
        self.gpu
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|backend| backend.graphics.pending())
    }

    #[cfg(feature = "vulkan")]
    pub fn graphics_persistence_stats(&self) -> Option<crate::gpu::pipelines::PersistenceStats> {
        self.gpu
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|backend| backend.graphics.persistence_stats())
    }

    /// Collect cache diagnostics away from queue submission.
    #[cfg(feature = "vulkan")]
    pub fn take_graphics_cache_diagnostics(&self) -> Vec<String> {
        self.gpu
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|backend| backend.graphics.take_diagnostics())
            .unwrap_or_default()
    }

    /// Explicitly forget a failed compilation, retaining all successful requests.
    #[cfg(feature = "vulkan")]
    pub fn retry_graphics_pipeline(
        &self,
        stages: &[Vec<u32>],
        input: crate::gpu::graphics::VertexInput,
        topology: crate::gpu::graphics::PrimitiveTopology,
    ) -> Result<bool, String> {
        self.gpu
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_mut()
            .ok_or("Vulkan backend is unavailable")?
            .graphics
            .retry_failed(stages, input, topology)
    }

    pub fn set_shader_translator(&self, translator: Option<Arc<dyn ShaderTranslator>>) {
        *self
            .shader_translator
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = translator;
    }

    pub fn shader_translator(&self) -> Option<Arc<dyn ShaderTranslator>> {
        self.shader_translator
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn set_shader_translation_enabled(&self, enabled: bool) {
        self.shader_translation_enabled
            .store(enabled, Ordering::Relaxed);
    }

    /// Set the directory for optional translated shader dumps. `None` disables
    /// dumping. The directory receives output derived from the observed
    /// program's shaders, never the original shader bytes.
    pub fn set_shader_dump_directory(&self, directory: Option<&Path>) {
        *self
            .shader_dump_directory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = directory.map(Path::to_path_buf);
    }

    pub(crate) fn shader_dump_directory(&self) -> Option<PathBuf> {
        self.shader_dump_directory
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(crate) fn shader_translation_enabled(&self) -> bool {
        self.shader_translation_enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn record_shader_translation_error(&self, error: String) {
        *self
            .shader_translation_errors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(error)
            .or_default() += 1;
    }

    pub(crate) fn record_successful_shader_translation(&self, words: &[u32]) {
        self.successful_shader_translations
            .fetch_add(1, Ordering::Relaxed);
        self.distinct_shader_outputs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(hash_bytes(
                &words
                    .iter()
                    .flat_map(|word| word.to_le_bytes())
                    .collect::<Vec<_>>(),
            ));
    }

    pub(crate) fn add_pending_shader(&self, program: u64, pending: PendingShader) {
        self.pending_shaders
            .lock()
            .unwrap()
            .entry(program)
            .or_default()
            .push(pending);
    }

    pub(crate) fn pending_shaders(&self, program: u64) -> Vec<PendingShader> {
        self.pending_shaders
            .lock()
            .unwrap()
            .get(&program)
            .cloned()
            .unwrap_or_default()
    }
    pub(crate) fn pending_programs(&self) -> Vec<u64> {
        self.pending_shaders
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect()
    }

    pub(crate) fn replace_pending_shader(&self, program: u64, record_index: usize) {
        if let Some(entries) = self.pending_shaders.lock().unwrap().get_mut(&program) {
            entries.retain(|entry| entry.record_index != record_index);
        }
    }

    pub(crate) fn fail_pending_shader(&self, program: u64, record_index: usize) {
        if let Some(entry) = self
            .pending_shaders
            .lock()
            .unwrap()
            .get_mut(&program)
            .and_then(|entries| {
                entries
                    .iter_mut()
                    .find(|entry| entry.record_index == record_index)
            })
        {
            entry.failures = entry.failures.saturating_add(1);
        }
    }

    pub(crate) fn record_late_read(&self, point: &'static str) {
        *self
            .shader_late_reads
            .lock()
            .unwrap()
            .entry(point)
            .or_default() += 1;
    }
    pub(crate) fn record_shader_zero_header(&self, zero: bool) {
        if zero {
            self.shader_zero_header.fetch_add(1, Ordering::Relaxed)
        } else {
            self.shader_header_without_code
                .fetch_add(1, Ordering::Relaxed)
        };
    }

    pub(crate) fn next_shader_dump(&self, bytes: &[u8]) -> Option<(u64, u64)> {
        let hash = hash_bytes(bytes);
        let mut hashes = self
            .shader_dump_hashes
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !hashes.insert(hash) {
            return None;
        }
        let sequence = self.shader_dump_sequence.fetch_add(1, Ordering::Relaxed);
        Some((sequence, hash))
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

    pub(crate) fn next_counter_report(&self) -> u64 {
        self.counter_reports.fetch_add(1, Ordering::Relaxed) << 10
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
                .record_call(self.host.as_ref(), &self.objects, registers)
        } else {
            None
        };
        PENDING.set(snapshot.map(|snapshot| (function.0, snapshot, *registers)));
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
            .filter(|(pending, _, _)| *pending == function.0)
            .map(|(_, snapshot, registers)| (snapshot, registers));
        if let Some((_, call_registers)) = snapshot.as_ref() {
            self.observe_pool_return(function, call_registers, registers);
        }
        self.shape(function).record_return(
            self.host.as_ref(),
            registers,
            snapshot.as_ref().map(|(snapshot, _)| snapshot),
        );
        Status::Ok
    }

    fn observe_pool_return(
        &self,
        function: FunctionId,
        call_registers: &Registers,
        return_registers: &Registers,
    ) {
        let Some(name) = functions::all()
            .find(|(id, _)| *id == function)
            .map(|(_, name)| name)
        else {
            return;
        };
        if name == "nvnMemoryPoolGetBufferAddress" {
            let pool = call_registers.x[0];
            let gpu_address = return_registers.x[0];
            self.objects.update(pool, |object| {
                if let crate::api::Object::MemoryPool {
                    observed_gpu_address: base,
                    ..
                } = object
                {
                    *base = Some(gpu_address);
                }
            });
        }
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
        let pending_shaders = self
            .pending_shaders
            .lock()
            .unwrap()
            .values()
            .map(|entries| entries.iter().filter(|entry| entry.failures < 64).count() as u64)
            .sum();
        let mut shader_translation_errors = self.shader_translation_errors.lock().unwrap().clone();
        if pending_shaders > 0 {
            *shader_translation_errors
                .entry("no code after header".into())
                .or_default() += pending_shaders;
        }
        Census {
            functions,
            unknown_requests,
            successful_shader_translations: self
                .successful_shader_translations
                .load(Ordering::Relaxed),
            distinct_shader_outputs: self
                .distinct_shader_outputs
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .len(),
            shader_translation_errors,
            pending_shaders,
            shader_late_reads: self.shader_late_reads.lock().unwrap().clone(),
            shader_zero_header: self.shader_zero_header.load(Ordering::Relaxed),
            shader_header_without_code: self.shader_header_without_code.load(Ordering::Relaxed),
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
    successful_shader_translations: u64,
    distinct_shader_outputs: usize,
    shader_translation_errors: BTreeMap<String, u64>,
    pending_shaders: u64,
    shader_late_reads: BTreeMap<&'static str, u64>,
    shader_zero_header: u64,
    shader_header_without_code: u64,
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
        for (error, count) in &self.shader_translation_errors {
            writeln!(f, "shader-translation-error {count} {error}")?;
        }
        for (point, count) in &self.shader_late_reads {
            writeln!(f, "shader-late-read {point} {count}")?;
        }
        writeln!(f, "shader-still-pending {}", self.pending_shaders)?;
        writeln!(f, "shader-zero-header {}", self.shader_zero_header)?;
        writeln!(
            f,
            "shader-header-without-code {}",
            self.shader_header_without_code
        )?;
        writeln!(
            f,
            "shader-translations-successful {} distinct-outputs {}",
            self.successful_shader_translations, self.distinct_shader_outputs
        )?;
        Ok(())
    }
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct FakeTranslator;

    impl ShaderTranslator for FakeTranslator {
        fn translate(&self, stage: ShaderStage, code: &[u8]) -> Result<Vec<u32>, String> {
            Ok(vec![stage, code.len() as u32])
        }
    }

    #[test]
    fn shader_translator_can_be_registered_and_called() {
        let instance = Instance::new();
        assert!(instance.shader_translator().is_none());
        instance.set_shader_translator(Some(Arc::new(FakeTranslator)));
        let translator = instance.shader_translator().expect("translator");
        assert_eq!(translator.translate(7, &[1, 2, 3]), Ok(vec![7, 3]));
        instance.set_shader_translator(None);
        assert!(instance.shader_translator().is_none());
    }

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
        assert!(
            text.contains("shader-translations-successful 0 distinct-outputs 0\n"),
            "{text}"
        );
        assert_eq!(text.lines().count(), 9, "{text}");
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

    #[test]
    fn observed_pool_address_is_used_for_shader_resolution() {
        let instance = Instance::new();
        let defaults = FunctionId(
            functions::lookup("nvnMemoryPoolBuilderSetDefaults")
                .unwrap()
                .0,
        );
        let storage = FunctionId(
            functions::lookup("nvnMemoryPoolBuilderSetStorage")
                .unwrap()
                .0,
        );
        let initialize = FunctionId(functions::lookup("nvnMemoryPoolInitialize").unwrap().0);
        let address = FunctionId(
            functions::lookup("nvnMemoryPoolGetBufferAddress")
                .unwrap()
                .0,
        );

        let mut registers = Registers {
            x: [20, 0, 0, 0, 0, 0, 0, 0],
            ..Registers::default()
        };
        assert_eq!(instance.call(defaults, &mut registers), Status::Ok);
        registers.x[0] = 20;
        registers.x[1] = 0x1000;
        registers.x[2] = 0x100;
        assert_eq!(instance.call(storage, &mut registers), Status::Ok);
        registers.x = [21, 20, 0, 0, 0, 0, 0, 0];
        assert_eq!(instance.call(initialize, &mut registers), Status::Ok);

        let call = Registers {
            x: [21, 0, 0, 0, 0, 0, 0, 0],
            ..Registers::default()
        };
        let result = Registers {
            x: [0x9000, 0, 0, 0, 0, 0, 0, 0],
            ..Registers::default()
        };
        let mut call_registers = call;
        assert_eq!(instance.call(address, &mut call_registers), Status::Ok);
        instance.returned(address, &result);

        assert_eq!(
            instance.objects.resolve_gpu_address(0x9080),
            Ok(crate::api::GpuAddress {
                pool: 21,
                offset: 0x80,
                remaining: 0x80,
                program_address: 0x1080,
            })
        );
    }
}
