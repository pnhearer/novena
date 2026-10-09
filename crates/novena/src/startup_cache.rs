//! Startup translation loading and owned background work. Provenance: 0031.
pub use crate::workers::{
    PipelineRequest as TranslationRequest, PipelineStatus as TranslationStatus,
};
use crate::{workers::AsyncPipelines, ShaderStage, ShaderTranslator};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

const MAX_RECORD: u64 = 64 * 1024 * 1024;
const MAX_INDEX: u64 = 256 * 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Explicit host translation inputs. The host adapter derives the exact upstream
/// identity and reports absent required state with None. No guest enums are inferred.
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct TranslationContext {
    /// Translator target generation included in the cache identity.
    pub generation: u8,
    /// Whether translation may assume a zero global-address delta.
    pub global_delta_zero: bool,
    /// Optional explicit geometry input state used by the translator.
    pub geometry_input: Option<u8>,
    /// Optional explicit tessellation state used by the translator.
    pub tessellation: Option<[u8; 3]>,
}

/// Owned translated module, cache identity, and execution requirements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranslatedShader {
    /// Exact translator cache identity as lowercase hexadecimal.
    pub key: String,
    /// Translated SPIR-V module words.
    pub words: Vec<u32>,
    /// Whether pipeline creation must require 32-lane subgroups.
    pub requires_subgroup_size_32: bool,
}

/// Host-owned persistence directory, translation inputs, and bounded worker settings.
#[derive(Clone, Debug)]
pub struct StartupCacheConfig {
    /// A private host-owned directory. The index is loaded during construction.
    pub directory: PathBuf,
    /// Translation inputs included in the cache identity.
    pub context: TranslationContext,
    /// Number of owned compilation workers.
    pub workers: usize,
    /// Maximum number of waiting jobs in the bounded channel.
    pub queue_capacity: usize,
    /// Explicit graphics recipes to preload in addition to saved recipes.
    #[cfg(feature = "vulkan")]
    pub pipelines: Vec<PipelineRecipe>,
}
impl StartupCacheConfig {
    /// Create a default translation context with two workers and capacity 64.
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            context: TranslationContext::default(),
            workers: 2,
            queue_capacity: 64,
            #[cfg(feature = "vulkan")]
            pipelines: Vec::new(),
        }
    }
}

/// Cumulative startup loading, translation, and scheduling counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StartupCacheStats {
    /// Validated shader records loaded into the startup index.
    pub loaded: u64,
    /// Counted registration lookups with a completed in-memory entry.
    pub hits: u64,
    /// Counted registration lookups without a completed in-memory entry.
    pub misses: u64,
    /// Registrations whose translator key required unavailable state.
    pub state_missing: u64,
    /// Successful worker translations.
    pub translated: u64,
    /// Summed worker translation time in nanoseconds, including failures.
    pub translation_nanoseconds: u64,
    /// Requests rejected because the bounded queue was full.
    pub queue_full: u64,
    /// Invalid cache records encountered while loading.
    pub invalid: u64,
    /// Saved or explicit graphics recipes queued for compilation.
    pub pipelines_queued: u64,
    /// Queued or compiling translation requests at the time of the snapshot.
    pub pending: u64,
}

/// Fully interpreted host state. Keys identify all stages of one draw. A shader
/// header alone cannot supply this recipe. Layout changes invalidate revision 1.
#[cfg(feature = "vulkan")]
#[derive(Clone, Debug, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PipelineRecipe {
    /// Exact cache identities of the stages used by this pipeline.
    pub keys: Vec<String>,
    /// Complete vertex input layout for the graphics pipeline.
    pub input: crate::gpu::graphics::VertexInput,
    /// Primitive assembly selected for this graphics pipeline.
    pub topology: crate::gpu::graphics::PrimitiveTopology,
    /// Complete interpreted graphics pipeline state.
    pub state: crate::gpu::graphics::DrawPipelineState,
    /// Whether uniform banks use storage buffers instead of uniform buffers.
    pub storage: bool,
}

#[derive(Clone)]
enum Job {
    Translate {
        key: String,
        stage: ShaderStage,
        code: Vec<u8>,
        context: TranslationContext,
    },
    #[cfg(feature = "vulkan")]
    Recipe(Box<PipelineRecipe>),
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Translate { key: left, .. }, Self::Translate { key: right, .. }) => {
                left == right
            }
            #[cfg(feature = "vulkan")]
            (Self::Recipe(left), Self::Recipe(right)) => left == right,
            #[cfg(feature = "vulkan")]
            _ => false,
        }
    }
}
impl Eq for Job {}
impl std::hash::Hash for Job {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Translate { key, .. } => {
                0_u8.hash(state);
                key.hash(state);
            }
            #[cfg(feature = "vulkan")]
            Self::Recipe(recipe) => {
                1_u8.hash(state);
                recipe.hash(state);
            }
        }
    }
}

struct Shared {
    entries: HashMap<String, Arc<TranslatedShader>>,
    stats: StartupCacheStats,
    diagnostics: Vec<String>,
}

pub(crate) struct StartupCache {
    context: TranslationContext,
    translator: Arc<dyn ShaderTranslator>,
    shared: Arc<Mutex<Shared>>,
    pool: AsyncPipelines<Job, TranslatedShader>,
    #[cfg(feature = "vulkan")]
    recipes: Vec<PipelineRecipe>,
}

fn valid_key(key: &str) -> bool {
    key.len() == 64
        && key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn read_record(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|_| "cache record open failed")?;
    let metadata = file
        .metadata()
        .map_err(|_| "cache record metadata failed")?;
    if !metadata.is_file() || metadata.len() > MAX_RECORD {
        return Err("cache record is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RECORD + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cache record read failed")?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err("cache record too large".into());
    }
    Ok(bytes)
}
fn decode(key: &str, bytes: &[u8]) -> Result<TranslatedShader, String> {
    if !valid_key(key)
        || bytes.len() < 125
        || &bytes[..8] != b"SBOXC007"
        || &bytes[8..72] != key.as_bytes()
        || bytes[104] > 1
        || !(bytes.len() - 105).is_multiple_of(4)
        || Sha256::digest(&bytes[104..])[..] != bytes[72..104]
    {
        return Err("invalid translation cache record".into());
    }
    let words: Vec<_> = bytes[105..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    if words[0] != 0x0723_0203 {
        return Err("invalid cached module header".into());
    }
    Ok(TranslatedShader {
        key: key.into(),
        words,
        requires_subgroup_size_32: bytes[104] == 1,
    })
}
fn encode(shader: &TranslatedShader) -> Result<Vec<u8>, String> {
    let mut payload = vec![u8::from(shader.requires_subgroup_size_32)];
    for word in &shader.words {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    let mut bytes = b"SBOXC007".to_vec();
    bytes.extend_from_slice(shader.key.as_bytes());
    bytes.extend_from_slice(&Sha256::digest(&payload));
    bytes.extend_from_slice(&payload);
    decode(&shader.key, &bytes)?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err("translated module too large".into());
    }
    Ok(bytes)
}
fn publish(directory: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|_| "cache directory creation failed")?;
    let (path, mut file) = loop {
        let path = directory.join(format!(
            ".{}.{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("cache temporary file creation failed".into()),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&path, directory.join(name))?;
        File::open(directory)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result.map_err(|_| "cache publication failed".into())
}

impl StartupCache {
    pub(crate) fn open(
        config: StartupCacheConfig,
        translator: Arc<dyn ShaderTranslator>,
    ) -> Result<Self, String> {
        let mut shared = Shared {
            entries: HashMap::new(),
            stats: StartupCacheStats::default(),
            diagnostics: Vec::new(),
        };
        #[cfg(feature = "vulkan")]
        let mut recipes = config.pipelines;
        let mut total = 0;
        match fs::read_dir(&config.directory) {
            Ok(files) => {
                for file in files {
                    let Ok(file) = file else {
                        shared.diagnostics.push("cache index read failed".into());
                        continue;
                    };
                    let path = file.path();
                    let extension = path.extension().and_then(|e| e.to_str());
                    if extension != Some("sbc") && extension != Some("recipe") {
                        continue;
                    }
                    // Skip symlinks, including links to special files, before opening.
                    if !file.file_type().is_ok_and(|t| t.is_file()) {
                        shared.stats.invalid += 1;
                        continue;
                    }
                    let loaded = read_record(&path).and_then(|bytes| {
                        total += bytes.len() as u64;
                        if total > MAX_INDEX {
                            return Err("cache index memory limit exceeded".into());
                        }
                        if extension == Some("sbc") {
                            let key = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                            let shader = decode(key, &bytes)?;
                            shared.entries.insert(key.into(), Arc::new(shader));
                            shared.stats.loaded += 1;
                        }
                        #[cfg(feature = "vulkan")]
                        if extension == Some("recipe") {
                            let (revision, recipe): (u32, PipelineRecipe) =
                                serde_json::from_slice(&bytes)
                                    .map_err(|_| "invalid pipeline recipe")?;
                            if revision != 1 || !recipe.keys.iter().all(|k| valid_key(k)) {
                                return Err("invalid pipeline recipe revision or key".into());
                            }
                            recipes.push(recipe);
                        }
                        Ok(())
                    });
                    if let Err(error) = loaded {
                        shared.stats.invalid += 1;
                        shared.diagnostics.push(error);
                    }
                    if total > MAX_INDEX {
                        break;
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => shared
                .diagnostics
                .push("cache directory read failed".into()),
        }
        let shared = Arc::new(Mutex::new(shared));
        let worker_shared = shared.clone();
        let worker_translator = translator.clone();
        let directory = config.directory;
        let pool = AsyncPipelines::new(
            HashMap::new(),
            Default::default(),
            config.workers,
            config.queue_capacity,
            move |job: &Job| match job {
                Job::Translate {
                    key,
                    stage,
                    code,
                    context,
                } => {
                    let start = Instant::now();
                    let result = worker_translator.translate_for_context(*stage, code, context);
                    let elapsed = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
                    worker_shared.lock().unwrap().stats.translation_nanoseconds += elapsed;
                    let mut shader = result?;
                    shader.key = key.clone();
                    let bytes = encode(&shader)?;
                    let shader = Arc::new(shader);
                    {
                        let mut shared = worker_shared.lock().unwrap();
                        shared.stats.translated += 1;
                        shared.entries.insert(key.clone(), shader.clone());
                    }
                    if let Err(error) = publish(&directory, &format!("{key}.sbc"), &bytes) {
                        worker_shared.lock().unwrap().diagnostics.push(error);
                    }
                    Ok(shader)
                }
                #[cfg(feature = "vulkan")]
                Job::Recipe(recipe) => {
                    let bytes = serde_json::to_vec(&(1, recipe))
                        .map_err(|_| "pipeline recipe encoding failed")?;
                    let name = format!("{:x}.recipe", Sha256::digest(&bytes));
                    if let Err(error) = publish(&directory, &name, &bytes) {
                        worker_shared.lock().unwrap().diagnostics.push(error);
                    }
                    Ok(Arc::new(TranslatedShader {
                        key: String::new(),
                        words: Vec::new(),
                        requires_subgroup_size_32: false,
                    }))
                }
            },
        )?;
        Ok(Self {
            context: config.context,
            translator,
            shared,
            pool,
            #[cfg(feature = "vulkan")]
            recipes,
        })
    }

    pub(crate) fn request(
        &mut self,
        stage: ShaderStage,
        code: &[u8],
        count_lookup: bool,
    ) -> Result<Option<TranslationRequest<TranslatedShader>>, String> {
        self.request_in_context(stage, code, &self.context.clone(), count_lookup)
    }

    pub(crate) fn context(&self) -> TranslationContext {
        self.context.clone()
    }

    pub(crate) fn request_in_context(
        &mut self,
        stage: ShaderStage,
        code: &[u8],
        context: &TranslationContext,
        count_lookup: bool,
    ) -> Result<Option<TranslationRequest<TranslatedShader>>, String> {
        self.request_with_admission(stage, code, context, count_lookup, false)
    }

    #[cfg(feature = "vulkan")]
    pub(crate) fn request_blocking(
        &mut self,
        stage: ShaderStage,
        code: &[u8],
        context: &TranslationContext,
    ) -> Result<Option<TranslationRequest<TranslatedShader>>, String> {
        self.request_with_admission(stage, code, context, false, true)
    }

    fn request_with_admission(
        &mut self,
        stage: ShaderStage,
        code: &[u8],
        context: &TranslationContext,
        count_lookup: bool,
        blocking: bool,
    ) -> Result<Option<TranslationRequest<TranslatedShader>>, String> {
        let Some(key) = self
            .translator
            .translation_cache_key(stage, code, context)?
        else {
            if count_lookup {
                self.shared.lock().unwrap().stats.state_missing += 1;
            }
            return Err("translation requires missing state".into());
        };
        if !valid_key(&key) {
            return Err("translator returned invalid cache key".into());
        }
        {
            let mut shared = self.shared.lock().unwrap();
            if let Some(shader) = shared.entries.get(&key).cloned() {
                if count_lookup {
                    shared.stats.hits += 1;
                }
                return Ok(Some(TranslationRequest::ready(shader)));
            }
            if count_lookup {
                shared.stats.misses += 1;
            }
        }
        let job = Job::Translate {
            key,
            stage,
            code: code.to_vec(),
            context: context.clone(),
        };
        #[cfg(feature = "vulkan")]
        let result = if blocking {
            self.pool.request_blocking(job)
        } else {
            self.pool.request(job)
        };
        #[cfg(not(feature = "vulkan"))]
        let result = {
            let _ = blocking;
            self.pool.request(job)
        };
        match result {
            Ok(request) => Ok(Some(request)),
            Err(crate::workers::RequestError::QueueFull) => {
                if count_lookup {
                    self.shared.lock().unwrap().stats.queue_full += 1;
                }
                Ok(None)
            }
            Err(crate::workers::RequestError::Stopped) => Err("translation workers stopped".into()),
        }
    }

    pub(crate) fn set_context(&mut self, context: TranslationContext) {
        self.context = context;
    }

    pub(crate) fn stats(&self) -> StartupCacheStats {
        let mut stats = self.shared.lock().unwrap().stats;
        stats.pending = self.pool.pending() as u64;
        stats
    }
    pub(crate) fn diagnostics(&self) -> Vec<String> {
        std::mem::take(&mut self.shared.lock().unwrap().diagnostics)
    }
    #[cfg(feature = "vulkan")]
    pub(crate) fn remember(&mut self, recipe: PipelineRecipe) {
        if self.pool.request(Job::Recipe(Box::new(recipe))).is_err() {
            self.shared
                .lock()
                .unwrap()
                .diagnostics
                .push("pipeline recipe queue full".into());
        }
    }
    #[cfg(feature = "vulkan")]
    pub(crate) fn schedule(&mut self, graphics: &mut crate::gpu::graphics::GraphicsPipelines) {
        self.drain_recipes(|stages, recipe| graphics.queue_startup(stages, recipe));
    }

    #[cfg(feature = "vulkan")]
    fn drain_recipes(
        &mut self,
        mut enqueue: impl FnMut(&[&TranslatedShader], &PipelineRecipe) -> Result<bool, String>,
    ) {
        let mut entries = self.shared.lock().unwrap();
        self.recipes.retain(|recipe| {
            let Some(shaders) = recipe
                .keys
                .iter()
                .map(|k| entries.entries.get(k))
                .collect::<Option<Vec<_>>>()
            else {
                entries
                    .diagnostics
                    .push("pipeline recipe lacks translated stages".into());
                return false;
            };
            let stages: Vec<_> = shaders.into_iter().map(AsRef::as_ref).collect();
            match enqueue(&stages, recipe) {
                Ok(true) => {
                    entries.stats.pipelines_queued += 1;
                    false
                }
                Ok(false) => true,
                Err(error) => {
                    entries.diagnostics.push(error);
                    false
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{atomic::AtomicUsize, Condvar},
        time::Duration,
    };

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "startup-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn config(&self) -> StartupCacheConfig {
            StartupCacheConfig::new(self.0.clone())
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn key(code: &[u8], context: &TranslationContext) -> String {
        let mut hash = Sha256::new();
        hash.update(b"original synthetic identity 1");
        hash.update(code);
        hash.update(format!("{context:?}").as_bytes());
        format!("{:x}", hash.finalize())
    }
    fn module(model: u32) -> Vec<u32> {
        vec![
            0x0723_0203,
            0x0001_0000,
            0,
            2,
            0,
            (5 << 16) | 15,
            model,
            1,
            0x6d,
            0,
        ]
    }
    struct Translator {
        calls: AtomicUsize,
        delay: Duration,
        gate: Option<Arc<(Mutex<bool>, Condvar)>>,
    }
    impl Translator {
        fn new(delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                delay,
                gate: None,
            })
        }
    }
    impl ShaderTranslator for Translator {
        fn translate(&self, _: ShaderStage, code: &[u8]) -> Result<Vec<u32>, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if let Some(gate) = &self.gate {
                let mut open = gate.0.lock().unwrap();
                while !*open {
                    open = gate.1.wait(open).unwrap();
                }
            }
            std::thread::sleep(self.delay);
            if code == b"fail" {
                return Err("original synthetic failure".into());
            }
            if code == b"panic" {
                panic!("original synthetic panic");
            }
            Ok(module(if code.first() == Some(&b'f') { 4 } else { 0 }))
        }
        fn translation_cache_key(
            &self,
            _: ShaderStage,
            code: &[u8],
            context: &TranslationContext,
        ) -> Result<Option<String>, String> {
            if code == b"missing" && context.geometry_input.is_none() {
                return Ok(None);
            }
            let identity_code = if code == b"alias" {
                b"vertex-old".as_slice()
            } else {
                code
            };
            Ok(Some(key(identity_code, context)))
        }
        fn translate_for_context(
            &self,
            stage: ShaderStage,
            code: &[u8],
            _: &TranslationContext,
        ) -> Result<TranslatedShader, String> {
            Ok(TranslatedShader {
                key: String::new(),
                words: self.translate(stage, code)?,
                requires_subgroup_size_32: code == b"subgroup",
            })
        }
    }
    fn request(cache: &mut StartupCache, code: &[u8]) -> TranslationRequest<TranslatedShader> {
        cache.request(0, code, true).unwrap().expect("queue space")
    }
    fn ready(request: &TranslationRequest<TranslatedShader>) -> Arc<TranslatedShader> {
        let start = Instant::now();
        loop {
            match request.poll() {
                TranslationStatus::Ready(shader) => return shader,
                TranslationStatus::Failed(error) => panic!("{error}"),
                _ => {
                    assert!(start.elapsed() < Duration::from_secs(10));
                    std::thread::yield_now();
                }
            }
        }
    }
    fn wait_failed(request: &TranslationRequest<TranslatedShader>) {
        let start = Instant::now();
        loop {
            if matches!(request.poll(), TranslationStatus::Failed(_)) {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::yield_now();
        }
    }

    #[test]
    fn restart_avoids_translation_and_preserves_subgroup_metadata() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::ZERO);
        let mut cold = StartupCache::open(directory.config(), translator.clone()).unwrap();
        let first = ready(&request(&mut cold, b"subgroup"));
        assert!(first.requires_subgroup_size_32);
        assert_eq!(cold.stats().misses, 1);
        drop(cold);
        let mut warm = StartupCache::open(directory.config(), translator.clone()).unwrap();
        let second = request(&mut warm, b"subgroup");
        assert!(matches!(second.poll(), TranslationStatus::Ready(_)));
        assert_eq!(*first, *ready(&second));
        assert_eq!(translator.calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            (
                warm.stats().loaded,
                warm.stats().hits,
                warm.stats().translated
            ),
            (1, 1, 0)
        );
        assert!(warm.diagnostics().is_empty());
    }

    #[test]
    fn corrupt_records_are_misses_and_atomic_repair_reopens() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::ZERO);
        let cache_key = key(b"vertex", &TranslationContext::default());
        for damage in 0..6 {
            let shader = TranslatedShader {
                key: cache_key.clone(),
                words: module(0),
                requires_subgroup_size_32: false,
            };
            let mut bytes = encode(&shader).unwrap();
            match damage {
                0 => bytes.truncate(100),
                1 => bytes[0] ^= 1,
                2 => bytes[8] ^= 1,
                3 => bytes[72] ^= 1,
                4 => {
                    bytes[104] = 2;
                    let digest = Sha256::digest(&bytes[104..]);
                    bytes[72..104].copy_from_slice(&digest);
                }
                _ => {
                    bytes[105] = 0;
                    let digest = Sha256::digest(&bytes[104..]);
                    bytes[72..104].copy_from_slice(&digest);
                }
            }
            fs::write(directory.0.join(format!("{cache_key}.sbc")), bytes).unwrap();
            fs::write(directory.0.join(".partial.tmp"), b"partial").unwrap();
            let mut cold = StartupCache::open(directory.config(), translator.clone()).unwrap();
            assert_eq!((cold.stats().invalid, cold.stats().loaded), (1, 0));
            assert!(!cold.diagnostics().is_empty());
            assert_eq!(ready(&request(&mut cold, b"vertex")).words, module(0));
            drop(cold);
            let mut warm = StartupCache::open(directory.config(), translator.clone()).unwrap();
            ready(&request(&mut warm, b"vertex"));
            assert_eq!((warm.stats().hits, warm.stats().invalid), (1, 0));
        }
    }

    #[test]
    fn bounded_workers_share_duplicates_retry_overflow_and_keep_replacements_owned() {
        let directory = Directory::new();
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let translator = Arc::new(Translator {
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            gate: Some(gate.clone()),
        });
        let mut config = directory.config();
        config.workers = 1;
        config.queue_capacity = 1;
        let mut cache = StartupCache::open(config, translator.clone()).unwrap();
        let original = request(&mut cache, b"vertex-old");
        let deadline = Instant::now() + Duration::from_secs(10);
        while translator.calls.load(Ordering::Relaxed) == 0 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let duplicate = request(&mut cache, b"vertex-old");
        assert_eq!(original, duplicate);
        let alias = request(&mut cache, b"alias");
        assert_eq!(original, alias);
        let replacement = request(&mut cache, b"fragment-new");
        assert!(cache.request(0, b"overflow", true).unwrap().is_none());
        assert_eq!(cache.stats().queue_full, 1);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert_eq!(
            ready(&replacement).key,
            key(b"fragment-new", &TranslationContext::default())
        );
        assert_eq!(
            ready(&original).key,
            key(b"vertex-old", &TranslationContext::default())
        );
        let captured = cache.context();
        cache.set_context(TranslationContext {
            generation: 1,
            ..Default::default()
        });
        let retry = cache
            .request_in_context(0, b"overflow", &captured, false)
            .unwrap()
            .unwrap();
        assert_eq!(ready(&retry).key, key(b"overflow", &captured));
        assert_eq!(translator.calls.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn missing_state_and_failed_translation_publish_no_entry() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::ZERO);
        let mut cache = StartupCache::open(directory.config(), translator.clone()).unwrap();
        assert!(cache
            .request(0, b"missing", true)
            .unwrap_err()
            .contains("missing state"));
        wait_failed(&request(&mut cache, b"fail"));
        wait_failed(&request(&mut cache, b"panic"));
        assert_eq!(
            (cache.stats().state_missing, cache.stats().translated),
            (1, 0)
        );
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
        drop(cache);
        let mut config = directory.config();
        config.context.geometry_input = Some(1);
        let mut known = StartupCache::open(config, translator).unwrap();
        ready(&request(&mut known, b"missing"));
        assert_eq!(known.stats().translated, 1);
    }

    #[test]
    fn context_separation_and_unwritable_cache_preserve_owned_results() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::ZERO);
        let mut first = StartupCache::open(directory.config(), translator.clone()).unwrap();
        ready(&request(&mut first, b"vertex"));
        drop(first);
        let mut config = directory.config();
        config.context.generation = 1;
        let mut changed = StartupCache::open(config, translator.clone()).unwrap();
        ready(&request(&mut changed, b"vertex"));
        assert_eq!(changed.stats().misses, 1);
        drop(changed);
        let blocked = directory.0.join("blocked");
        fs::write(&blocked, b"regular file").unwrap();
        let mut unavailable =
            StartupCache::open(StartupCacheConfig::new(blocked), translator).unwrap();
        ready(&request(&mut unavailable, b"vertex"));
        assert!(unavailable
            .diagnostics()
            .iter()
            .any(|s| s == "cache publication failed" || s == "cache directory creation failed"));
        drop(unavailable); // Joins the publication work as well as translation.
    }

    #[test]
    fn bounds_special_files_and_invalid_worker_configuration_are_explicit() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::ZERO);
        let oversized = directory
            .0
            .join(format!("{}.sbc", key(b"large", &Default::default())));
        File::create(oversized)
            .unwrap()
            .set_len(MAX_RECORD + 1)
            .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            ".",
            directory
                .0
                .join(format!("{}.sbc", key(b"link", &Default::default()))),
        )
        .unwrap();
        let cache = StartupCache::open(directory.config(), translator.clone()).unwrap();
        assert!(cache.stats().invalid >= 1);
        assert_eq!(cache.stats().loaded, 0);
        drop(cache);
        let mut config = directory.config();
        config.workers = 0;
        assert!(StartupCache::open(config, translator.clone()).is_err());
        let mut config = directory.config();
        config.queue_capacity = 0;
        assert!(StartupCache::open(config, translator).is_err());
    }

    #[test]
    fn concurrent_instances_publish_complete_records() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::ZERO);
        let mut first = StartupCache::open(directory.config(), translator.clone()).unwrap();
        let mut second = StartupCache::open(directory.config(), translator.clone()).unwrap();
        let a = request(&mut first, b"vertex");
        let b = request(&mut second, b"vertex");
        assert_eq!(*ready(&a), *ready(&b));
        drop(first);
        drop(second);
        let mut reopened = StartupCache::open(directory.config(), translator.clone()).unwrap();
        ready(&request(&mut reopened, b"vertex"));
        assert_eq!(reopened.stats().hits, 1);
        assert_eq!(translator.calls.load(Ordering::Relaxed), 2);
        assert!(reopened.diagnostics().is_empty());
    }

    #[cfg(feature = "vulkan")]
    fn recipe(vertex: String, fragment: String) -> PipelineRecipe {
        PipelineRecipe {
            keys: vec![vertex, fragment],
            input: crate::gpu::graphics::VertexInput {
                bindings: vec![],
                attributes: vec![],
            },
            topology: crate::gpu::graphics::PrimitiveTopology::TriangleList,
            state: Default::default(),
            storage: false,
        }
    }

    #[cfg(feature = "vulkan")]
    #[test]
    fn reopened_recipes_forward_subgroup_metadata() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::ZERO);
        let mut cold = StartupCache::open(directory.config(), translator.clone()).unwrap();
        let vertex = ready(&request(&mut cold, b"vertex"));
        let subgroup = ready(&request(&mut cold, b"subgroup"));
        cold.remember(recipe(vertex.key.clone(), subgroup.key.clone()));
        drop(cold);
        let mut warm = StartupCache::open(directory.config(), translator).unwrap();
        let mut received = false;
        warm.drain_recipes(|stages, _| {
            assert!(!stages[0].requires_subgroup_size_32);
            assert!(stages[1].requires_subgroup_size_32);
            received = true;
            Ok(true)
        });
        assert!(received);
        assert_eq!(warm.stats().pipelines_queued, 1);
        assert!(warm.diagnostics().is_empty());
    }

    #[cfg(feature = "vulkan")]
    #[test]
    fn recipes_reopen_validate_and_retry_a_full_pipeline_queue() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::ZERO);
        let mut cache = StartupCache::open(directory.config(), translator.clone()).unwrap();
        let vertex = ready(&request(&mut cache, b"vertex"));
        let fragment = ready(&request(&mut cache, b"fragment"));
        let original = recipe(vertex.key.clone(), fragment.key.clone());
        cache.remember(original.clone());
        drop(cache);
        let mut warm = StartupCache::open(directory.config(), translator).unwrap();
        assert_eq!(warm.recipes, vec![original.clone()]);
        warm.drain_recipes(|_, _| Ok(false));
        assert_eq!(warm.recipes.len(), 1);
        warm.drain_recipes(|stages, recipe| {
            crate::gpu::graphics::Key::new(
                &stages.iter().map(|s| s.words.clone()).collect::<Vec<_>>(),
                recipe.input.clone(),
                recipe.topology,
                recipe.state,
                recipe.storage,
            )?;
            assert_eq!(recipe, &original);
            Ok(true)
        });
        assert!(warm.recipes.is_empty());
        assert_eq!(warm.stats().pipelines_queued, 1);
    }

    #[cfg(feature = "vulkan")]
    #[test]
    fn synthetic_cold_start_reports_ready_draw_and_translation_time_avoided() {
        let directory = Directory::new();
        let translator = Translator::new(Duration::from_millis(8));
        let cold_start = Instant::now();
        let mut cold = StartupCache::open(directory.config(), translator.clone()).unwrap();
        let vertex = ready(&request(&mut cold, b"vertex"));
        let fragment = ready(&request(&mut cold, b"fragment"));
        let recipe = recipe(vertex.key.clone(), fragment.key.clone());
        let compiler = |_: &PipelineRecipe| {
            std::thread::sleep(Duration::from_millis(2));
            Ok(Arc::new(module(0)))
        };
        let mut pipelines =
            AsyncPipelines::new(HashMap::new(), Default::default(), 1, 4, compiler).unwrap();
        let draw = pipelines.request(recipe.clone()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !matches!(draw.poll(), crate::workers::PipelineStatus::Ready(_)) {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let cold_draw = cold_start.elapsed();
        cold.remember(recipe.clone());
        let cold_stats = cold.stats();
        drop(cold);
        drop(pipelines);
        let warm_start = Instant::now();
        let mut warm = StartupCache::open(directory.config(), translator.clone()).unwrap();
        let mut pipelines =
            AsyncPipelines::new(HashMap::new(), Default::default(), 1, 4, compiler).unwrap();
        warm.drain_recipes(|_, recipe| {
            pipelines
                .request(recipe.clone())
                .map_err(|e| format!("{e:?}"))?;
            Ok(true)
        });
        ready(&request(&mut warm, b"vertex"));
        ready(&request(&mut warm, b"fragment"));
        let draw = pipelines.request(recipe).unwrap();
        while !matches!(draw.poll(), crate::workers::PipelineStatus::Ready(_)) {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        let warm_draw = warm_start.elapsed();
        assert_eq!(translator.calls.load(Ordering::Relaxed), 2);
        assert_eq!(warm.stats().translation_nanoseconds, 0);
        assert_eq!(warm.stats().hits, 2);
        assert_eq!(pipelines.stats.misses, 1);
        println!("synthetic CPU draw readiness: cold_us={} warm_us={} translation_us_avoided={} translations_avoided=2", cold_draw.as_micros(), warm_draw.as_micros(), cold_stats.translation_nanoseconds / 1000);
    }
}
