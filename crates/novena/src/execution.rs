//! Ordered guest submission ownership. Provenance: 0034.
use crate::{api::RecordedCommand, instance::InstanceState, Status};
use std::sync::Arc;
#[cfg(feature = "vulkan")]
use std::sync::Mutex;
#[cfg(feature = "vulkan")]
use std::{
    sync::{mpsc, OnceLock},
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
    Submit(Vec<Vec<RecordedCommand>>),
    Drain(mpsc::SyncSender<Status>),
}
#[cfg(feature = "vulkan")]
struct Runtime {
    sender: Mutex<Option<mpsc::Sender<Message>>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
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
    pub fn submit(&self, buffers: Vec<Vec<RecordedCommand>>) -> Status {
        #[cfg(feature = "vulkan")]
        if self.enabled {
            let runtime = self
                .runtime
                .get_or_init(|| Runtime::start(self.state.clone()));
            return if runtime.send(Message::Submit(buffers)) {
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
        let worker = thread::Builder::new()
            .name("execution-owner".into())
            .spawn(move || {
                EXECUTION_OWNER.with(|owner| owner.set(Arc::as_ptr(&state) as usize));
                let mut status = Status::Ok;
                for message in receiver {
                    match message {
                        Message::Submit(buffers) if status != Status::InternalError => {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    crate::api::queue::execute(&state, buffers)
                                }))
                                .unwrap_or(Status::InternalError);
                            if status == Status::Ok || result == Status::InternalError {
                                status = result;
                            }
                        }
                        Message::Submit(_) => {}
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
        }
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
