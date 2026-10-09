//! Bounded owned pipeline requests. Evidence: provenance 0026 and 0027.
/// Cumulative request reuse and insertion counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Requests that reused an existing entry.
    pub hits: u64,
    /// Requests that created a new entry.
    pub misses: u64,
}
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU8, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
};

/// A frame polls this state and skips the draw unless Ready is returned.
pub enum PipelineStatus<P> {
    /// The owned job is waiting for a worker.
    Queued,
    /// A worker is compiling the owned job.
    Compiling,
    /// Compilation completed and the shared result is available.
    Ready(Arc<P>),
    /// Compilation failed; the diagnostic stays available until explicit retry.
    Failed(String),
}

struct RequestState<P> {
    phase: AtomicU8,
    result: Mutex<Option<Result<Arc<P>, String>>>,
}

/// Owned completion handle. Replacing a program replaces its handle, so an old
/// completion cannot modify the current program or follow a guest pointer.
pub struct PipelineRequest<P>(Arc<RequestState<P>>);

impl<P> Clone for PipelineRequest<P> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<P> PipelineRequest<P> {
    /// Create an already completed request retaining the supplied shared result.
    pub fn ready(value: Arc<P>) -> Self {
        Self(Arc::new(RequestState {
            phase: AtomicU8::new(2),
            result: Mutex::new(Some(Ok(value))),
        }))
    }

    /// Return the current phase without waiting for compilation.
    pub fn poll(&self) -> PipelineStatus<P> {
        match self.0.phase.load(Ordering::Acquire) {
            0 => PipelineStatus::Queued,
            1 => PipelineStatus::Compiling,
            _ => match self.0.result.try_lock() {
                Ok(result) => match result.as_ref().expect("completed request has a result") {
                    Ok(pipeline) => PipelineStatus::Ready(pipeline.clone()),
                    Err(error) => PipelineStatus::Failed(error.clone()),
                },
                Err(_) => PipelineStatus::Compiling,
            },
        }
    }
}

/// Failure to enqueue a new owned compilation request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    /// The bounded job channel has no available slot.
    QueueFull,
    /// The worker channel has disconnected.
    Stopped,
}

struct CompileJob<K, P> {
    program: K,
    state: Arc<RequestState<P>>,
}

/// Bounded compiler pool. Construction and shutdown belong outside frame recording.
/// Requests and polling never wait for compilation, cache locks or disk operations.
pub(crate) struct AsyncPipelines<K, P> {
    requests: HashMap<K, PipelineRequest<P>>,
    sender: Option<mpsc::SyncSender<CompileJob<K, P>>>,
    workers: Vec<JoinHandle<()>>,
    pub(crate) stats: CacheStats,
}

impl<K: Eq + std::hash::Hash + Clone + Send + 'static, P: Send + Sync + 'static>
    AsyncPipelines<K, P>
{
    /// Converts a populated cache into a worker service, preserving ready pipelines.
    /// Queue capacity bounds waiting work; worker_count bounds active work.
    pub fn new(
        pipelines: HashMap<K, Arc<P>>,
        stats: CacheStats,
        worker_count: usize,
        queue_capacity: usize,
        compile: impl Fn(&K) -> Result<Arc<P>, String> + Send + Sync + 'static,
    ) -> Result<Self, String> {
        if worker_count == 0 || queue_capacity == 0 {
            return Err("compiler pool requires workers and queue capacity".into());
        }
        let (sender, receiver) = mpsc::sync_channel::<CompileJob<K, P>>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let compiler = Arc::new(compile);
        let mut result = Self {
            requests: pipelines
                .into_iter()
                .map(|(program, pipeline)| {
                    (
                        program,
                        PipelineRequest(Arc::new(RequestState {
                            phase: AtomicU8::new(2),
                            result: Mutex::new(Some(Ok(pipeline))),
                        })),
                    )
                })
                .collect(),
            sender: Some(sender),
            workers: Vec::new(),
            stats,
        };
        for index in 0..worker_count {
            let receiver = receiver.clone();
            let compiler = compiler.clone();
            let worker = thread::Builder::new()
                .name(format!("pipeline-{index}"))
                .spawn(move || loop {
                    let job = {
                        let receiver = receiver.lock().unwrap();
                        receiver.recv()
                    };
                    let Ok(job) = job else { break };
                    job.state.phase.store(1, Ordering::Release);
                    let compiled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        compiler(&job.program)
                    }))
                    .unwrap_or_else(|_| Err("pipeline compiler panicked".into()));
                    *job.state.result.lock().unwrap() = Some(compiled);
                    job.state.phase.store(2, Ordering::Release);
                })
                .map_err(|error| format!("start pipeline worker: {error}"))?;
            result.workers.push(worker);
        }
        Ok(result)
    }

    /// Duplicate requests share one result. QueueFull leaves no cache entry;
    /// skip this frame and retry later. Failed requests require explicit retry.
    pub fn request(&mut self, program: K) -> Result<PipelineRequest<P>, RequestError> {
        if let Some(request) = self.requests.get(&program) {
            self.stats.hits += 1;
            return Ok(request.clone());
        }
        let state = Arc::new(RequestState {
            phase: AtomicU8::new(0),
            result: Mutex::new(None),
        });
        self.sender
            .as_ref()
            .ok_or(RequestError::Stopped)?
            .try_send(CompileJob {
                program: program.clone(),
                state: state.clone(),
            })
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => RequestError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => RequestError::Stopped,
            })?;
        let request = PipelineRequest(state);
        self.requests.insert(program, request.clone());
        self.stats.misses += 1;
        Ok(request)
    }

    /// Forget only a failed request. Existing handles retain their original result.
    #[cfg(feature = "vulkan")]
    pub fn retry_failed(&mut self, program: &K) -> bool {
        if self
            .requests
            .get(program)
            .is_some_and(|request| matches!(request.poll(), PipelineStatus::Failed(_)))
        {
            self.requests.remove(program);
            true
        } else {
            false
        }
    }

    pub(crate) fn pending(&self) -> usize {
        self.requests
            .values()
            .filter(|request| {
                matches!(
                    request.poll(),
                    PipelineStatus::Queued | PipelineStatus::Compiling
                )
            })
            .count()
    }
}

impl<K, P> Drop for AsyncPipelines<K, P> {
    fn drop(&mut self) {
        self.sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl<P> std::fmt::Debug for PipelineRequest<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineRequest")
            .field("phase", &self.0.phase.load(Ordering::Acquire))
            .finish()
    }
}
impl<P> PartialEq for PipelineRequest<P> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl<P> Eq for PipelineRequest<P> {}
