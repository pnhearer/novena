//! Ordered guest submission ownership. Provenance: 0034.
use crate::{api::RecordedCommand, instance::InstanceState, Status};
use std::sync::Arc;
#[cfg(feature = "vulkan")]
use std::sync::Mutex;
#[cfg(feature = "vulkan")]
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, OnceLock,
    },
    thread,
};

#[cfg(feature = "vulkan")]
thread_local! {
    static EXECUTION_OWNER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
pub(crate) struct QueueExecutor {
    state: Arc<InstanceState>,
    #[cfg(feature = "vulkan")]
    enabled: bool,
    #[cfg(feature = "vulkan")]
    runtime: OnceLock<Runtime>,
}
#[cfg(feature = "vulkan")]
enum Message {
    Submit(u64, Vec<(u64, Vec<RecordedCommand>)>),
    Drain(mpsc::SyncSender<Status>),
}
#[cfg(feature = "vulkan")]
struct Runtime {
    sender: Mutex<Option<mpsc::Sender<Message>>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    submitted: AtomicU64,
    completed: Arc<AtomicU64>,
}
impl QueueExecutor {
    pub fn new(state: Arc<InstanceState>) -> Self {
        #[cfg(feature = "vulkan")]
        let enabled = state
            .gpu
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some();
        Self {
            #[cfg(feature = "vulkan")]
            enabled,
            state,
            #[cfg(feature = "vulkan")]
            runtime: OnceLock::new(),
        }
    }
    pub fn submit(&self, buffers: Vec<(u64, Vec<RecordedCommand>)>) -> Status {
        #[cfg(feature = "vulkan")]
        if self.enabled {
            let runtime = self
                .runtime
                .get_or_init(|| Runtime::start(self.state.clone()));
            return if runtime.submit(buffers) {
                Status::Ok
            } else {
                Status::InternalError
            };
        }
        crate::api::queue::execute(&self.state, buffers)
    }
    pub fn drain(&self) -> Status {
        #[cfg(feature = "vulkan")]
        if EXECUTION_OWNER.with(|owner| owner.get() == Arc::as_ptr(&self.state) as usize) {
            return Status::InternalError;
        }
        #[cfg(feature = "vulkan")]
        if let Some(runtime) = self.runtime.get() {
            if runtime.completed.load(Ordering::Acquire)
                == runtime.submitted.load(Ordering::Acquire)
            {
                return Status::Ok;
            }
            let (sender, receiver) = mpsc::sync_channel(1);
            if !runtime.send(Message::Drain(sender)) {
                return Status::InternalError;
            }
            return receiver.recv().unwrap_or(Status::InternalError);
        }
        Status::Ok
    }
    pub fn shutdown(&mut self) {
        #[cfg(feature = "vulkan")]
        if let Some(runtime) = self.runtime.get_mut() {
            runtime.shutdown();
        }
    }
}
#[cfg(feature = "vulkan")]
impl Runtime {
    fn start(state: Arc<InstanceState>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let completed = Arc::new(AtomicU64::new(0));
        let worker_completed = completed.clone();
        let worker = thread::Builder::new()
            .name("execution-owner".into())
            .spawn(move || {
                EXECUTION_OWNER.with(|owner| owner.set(Arc::as_ptr(&state) as usize));
                let mut status = Status::Ok;
                let mut consumed = 0;
                for message in receiver {
                    match message {
                        Message::Submit(ticket, buffers) if status != Status::InternalError => {
                            consumed = ticket;
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    crate::api::queue::execute(&state, buffers)
                                }))
                                .unwrap_or(Status::InternalError);
                            if status == Status::Ok || result == Status::InternalError {
                                status = result;
                            }
                        }
                        Message::Submit(ticket, _) => {
                            consumed = ticket;
                        }
                        Message::Drain(reply) => {
                            if state
                                .gpu
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .as_ref()
                                .is_some_and(|backend| !backend.finish())
                            {
                                status = Status::InternalError;
                            }
                            if status == Status::Ok {
                                worker_completed.store(consumed, Ordering::Release);
                            }
                            let _ = reply.send(status);
                            if status != Status::InternalError {
                                status = Status::Ok;
                            }
                        }
                    }
                }
                if let Some(backend) = state.gpu.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
                {
                    let _ = backend.finish();
                }
            })
            .expect("execution thread");
        Self {
            sender: Mutex::new(Some(sender)),
            worker: Mutex::new(Some(worker)),
            submitted: AtomicU64::new(0),
            completed,
        }
    }
    fn submit(&self, buffers: Vec<(u64, Vec<RecordedCommand>)>) -> bool {
        let sender = self.sender.lock().unwrap_or_else(|p| p.into_inner());
        let ticket = self.submitted.fetch_add(1, Ordering::AcqRel) + 1;
        sender
            .as_ref()
            .is_some_and(|sender| sender.send(Message::Submit(ticket, buffers)).is_ok())
    }
    fn send(&self, message: Message) -> bool {
        self.sender
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|sender| sender.send(message).is_ok())
    }
    fn shutdown(&mut self) {
        self.sender
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            let _ = worker.join();
        }
    }
}

#[cfg(all(test, feature = "vulkan"))]
mod tests {
    use super::*;
    use crate::{functions, Host, Instance, Registers};
    use std::time::Duration;

    #[test]
    fn completed_prefix_skips_empty_handoffs_and_reports_errors_once() {
        let state = Arc::new(InstanceState::new());
        let mut queue = QueueExecutor::new(state);
        queue.enabled = true;
        assert_eq!(
            queue.submit(vec![(
                0,
                vec![RecordedCommand::DrawArrays {
                    primitive: 0,
                    first: 0,
                    count: 0,
                }]
            )]),
            Status::Ok
        );
        assert_eq!(queue.drain(), Status::Unimplemented);
        assert_eq!(queue.drain(), Status::Ok);
        let runtime = queue.runtime.get().unwrap();
        assert_eq!(
            runtime.completed.load(Ordering::Acquire),
            runtime.submitted.load(Ordering::Acquire)
        );
        assert_eq!(queue.drain(), Status::Ok);
        assert_eq!(queue.submit(Vec::new()), Status::Ok);
        assert_eq!(queue.drain(), Status::Ok);
        queue.shutdown();
    }

    #[test]
    #[ignore = "requires a Vulkan device"]
    fn guest_submit_returns_while_execution_is_blocked() {
        let instance = unsafe { Instance::with_host(Host::default()) };
        assert!(
            instance.gpu.lock().unwrap().is_some(),
            "Vulkan device required"
        );
        let driver = instance.gpu.lock().unwrap();
        let submit = functions::all()
            .find(|(_, name)| name.ends_with("QueueSubmitCommands"))
            .unwrap()
            .0;
        let (sent, returned) = mpsc::sync_channel(1);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                sent.send(instance.call(submit, &mut Registers::default()))
                    .unwrap();
            });
            let result = returned.recv_timeout(Duration::from_secs(5));
            drop(driver);
            assert_eq!(result.unwrap(), Status::Ok);
        });
        assert_eq!(instance.queue.drain(), Status::Ok);
    }
}
