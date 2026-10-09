//! Worker-local pools and ordered queue ownership. Provenance: 0034.
use super::recording_device::Operation;
use ash::vk;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
#[derive(Clone, Default)]
pub(super) struct Completion(Arc<(Mutex<Option<bool>>, Condvar)>);
impl Completion {
    fn finish(&self, success: bool) {
        let (state, changed) = &*self.0;
        *state.lock().unwrap_or_else(|p| p.into_inner()) = Some(success);
        changed.notify_all();
    }
    pub fn wait(&self) -> Option<()> {
        let (state, changed) = &*self.0;
        let mut result = state.lock().unwrap_or_else(|p| p.into_inner());
        while result.is_none() {
            result = changed.wait(result).unwrap_or_else(|p| p.into_inner());
        }
        result.unwrap().then_some(())
    }
}
struct Job {
    ticket: u64,
    operations: Vec<Operation>,
    wait: Option<vk::Semaphore>,
    signal: Option<vk::Semaphore>,
    completion: Completion,
}
enum Payload {
    Recorded {
        command: vk::CommandBuffer,
        fence: vk::Fence,
        wait: Option<vk::Semaphore>,
        signal: Option<vk::Semaphore>,
    },
    Failed,
    Idle(mpsc::SyncSender<Result<(), vk::Result>>),
    Present {
        queue: vk::Queue,
        loader: ash::khr::swapchain::Device,
        swapchain: vk::SwapchainKHR,
        index: u32,
        waits: Vec<vk::Semaphore>,
        reply: mpsc::SyncSender<Result<bool, vk::Result>>,
    },
}
struct Ready {
    ticket: u64,
    payload: Payload,
    completion: Completion,
}
pub(super) struct Runtime {
    next: AtomicU64,
    jobs: Vec<mpsc::Sender<Job>>,
    ready: Option<mpsc::Sender<Ready>>,
    workers: Vec<JoinHandle<()>>,
    submit: Option<JoinHandle<()>>,
}
struct Pool {
    device: ash::Device,
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
}
impl Pool {
    fn new(device: ash::Device, family: u32) -> Option<Self> {
        let pool = unsafe {
            device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default().queue_family_index(family),
                    None,
                )
                .ok()?
        };
        let mut resources = Self {
            device,
            pool,
            command: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
        };
        resources.command = unsafe {
            resources
                .device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(1),
                )
                .ok()?[0]
        };
        resources.fence = unsafe {
            resources
                .device
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .ok()?
        };
        Some(resources)
    }
    fn record(&self, operations: &[Operation]) -> Option<()> {
        unsafe {
            self.device
                .reset_command_pool(self.pool, vk::CommandPoolResetFlags::empty())
                .ok()?;
            self.device
                .begin_command_buffer(
                    self.command,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .ok()?;
            self.device.cmd_pipeline_barrier(
                self.command,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[vk::MemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
                    .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)],
                &[],
                &[],
            );
            for operation in operations {
                operation(&self.device, self.command);
            }
            self.device.end_command_buffer(self.command).ok()?;
        }
        Some(())
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.pool, None);
        }
    }
}
impl Runtime {
    pub fn new(device: &ash::Device, queue: vk::Queue, family: u32) -> Self {
        let (ready, receiver) = mpsc::channel();
        let submit_device = device.clone();
        let submit = thread::Builder::new()
            .name("queue-submit".into())
            .spawn(move || submit_loop(submit_device, queue, receiver))
            .expect("submit thread");
        let count = thread::available_parallelism()
            .map_or(2, usize::from)
            .clamp(2, 8);
        let mut jobs = Vec::new();
        let mut workers = Vec::new();
        for index in 0..count {
            let (sender, receiver) = mpsc::channel::<Job>();
            jobs.push(sender);
            let ready = ready.clone();
            let device = device.clone();
            workers.push(
                thread::Builder::new()
                    .name(format!("command-worker-{index}"))
                    .spawn(move || {
                        let pool = Pool::new(device, family);
                        let mut healthy = pool.is_some();
                        for job in receiver {
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    if healthy {
                                        pool.as_ref()?.record(&job.operations)?;
                                        Some(())
                                    } else {
                                        None
                                    }
                                }))
                                .ok()
                                .flatten();
                            let payload = if result.is_some() {
                                let pool = pool.as_ref().unwrap();
                                Payload::Recorded {
                                    command: pool.command,
                                    fence: pool.fence,
                                    wait: job.wait,
                                    signal: job.signal,
                                }
                            } else {
                                Payload::Failed
                            };
                            let completion = job.completion.clone();
                            if ready
                                .send(Ready {
                                    ticket: job.ticket,
                                    payload,
                                    completion,
                                })
                                .is_err()
                            {
                                job.completion.finish(false);
                                break;
                            }
                            // One active buffer per private pool. Reset only after fence retirement.
                            healthy &= job.completion.wait().is_some();
                        }
                    })
                    .expect("command worker"),
            );
        }
        Self {
            next: AtomicU64::new(0),
            jobs,
            ready: Some(ready),
            workers,
            submit: Some(submit),
        }
    }
    pub fn enqueue(
        &self,
        operations: Vec<Operation>,
        wait: Option<vk::Semaphore>,
        signal: Option<vk::Semaphore>,
    ) -> Completion {
        let ticket = self.next.fetch_add(1, Ordering::Relaxed);
        let completion = Completion::default();
        let job = Job {
            ticket,
            operations,
            wait,
            signal,
            completion: completion.clone(),
        };
        if self.jobs[ticket as usize % self.jobs.len()]
            .send(job)
            .is_err()
        {
            let _ = self.ready.as_ref().unwrap().send(Ready {
                ticket,
                payload: Payload::Failed,
                completion: completion.clone(),
            });
        }
        completion
    }
    fn action(&self, payload: Payload) -> Result<(), vk::Result> {
        let ticket = self.next.fetch_add(1, Ordering::Relaxed);
        self.ready
            .as_ref()
            .unwrap()
            .send(Ready {
                ticket,
                payload,
                completion: Completion::default(),
            })
            .map_err(|_| vk::Result::ERROR_DEVICE_LOST)
    }
    pub fn wait_idle(&self) -> Result<(), vk::Result> {
        let (reply, result) = mpsc::sync_channel(1);
        self.action(Payload::Idle(reply))?;
        result.recv().unwrap_or(Err(vk::Result::ERROR_DEVICE_LOST))
    }
    pub fn present(
        &self,
        queue: vk::Queue,
        loader: &ash::khr::swapchain::Device,
        swapchain: vk::SwapchainKHR,
        index: u32,
        waits: &[vk::Semaphore],
    ) -> Result<bool, vk::Result> {
        let (reply, result) = mpsc::sync_channel(1);
        self.action(Payload::Present {
            queue,
            loader: loader.clone(),
            swapchain,
            index,
            waits: waits.to_vec(),
            reply,
        })?;
        result.recv().unwrap_or(Err(vk::Result::ERROR_DEVICE_LOST))
    }
    pub fn shutdown(&mut self) {
        self.jobs.clear();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        self.ready.take();
        if let Some(submit) = self.submit.take() {
            let _ = submit.join();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn submit_loop(device: ash::Device, queue: vk::Queue, receiver: mpsc::Receiver<Ready>) {
    let mut next = 0;
    let mut ready = BTreeMap::new();
    let mut pending: VecDeque<(vk::Fence, Completion)> = VecDeque::new();
    let mut failed = false;
    let mut disconnected = false;
    loop {
        let input = if pending.is_empty() && !disconnected {
            receiver
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        } else {
            receiver.recv_timeout(Duration::from_micros(100))
        };
        match input {
            Ok(item) => {
                ready.insert(item.ticket, item);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => disconnected = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while let Ok(item) = receiver.try_recv() {
            ready.insert(item.ticket, item);
        }
        while let Some(item) = ready.remove(&next) {
            next += 1;
            match item.payload {
                Payload::Recorded {
                    command,
                    fence,
                    wait,
                    signal,
                } if !failed => {
                    let commands = [command];
                    let waits: Vec<_> = wait.into_iter().collect();
                    let signals: Vec<_> = signal.into_iter().collect();
                    let stages = [vk::PipelineStageFlags::ALL_COMMANDS];
                    let mut info = vk::SubmitInfo::default()
                        .command_buffers(&commands)
                        .signal_semaphores(&signals);
                    if !waits.is_empty() {
                        info = info.wait_semaphores(&waits).wait_dst_stage_mask(&stages);
                    }
                    let result = unsafe {
                        device
                            .reset_fences(&[fence])
                            .and_then(|()| device.queue_submit(queue, &[info], fence))
                    };
                    if result.is_ok() {
                        pending.push_back((fence, item.completion));
                    } else {
                        failed = true;
                        unsafe {
                            let _ = device.device_wait_idle();
                        }
                        item.completion.finish(false);
                    }
                }
                Payload::Recorded { .. } | Payload::Failed => {
                    failed = true;
                    item.completion.finish(false);
                }
                Payload::Idle(reply) => {
                    let result = unsafe { device.device_wait_idle() };
                    failed |= result.is_err();
                    for (_, completion) in pending.drain(..) {
                        completion.finish(!failed);
                    }
                    let _ = reply.send(if failed {
                        Err(vk::Result::ERROR_DEVICE_LOST)
                    } else {
                        result
                    });
                    item.completion.finish(!failed);
                }
                Payload::Present {
                    queue,
                    loader,
                    swapchain,
                    index,
                    waits,
                    reply,
                } => {
                    let chains = [swapchain];
                    let indices = [index];
                    let info = vk::PresentInfoKHR::default()
                        .wait_semaphores(&waits)
                        .swapchains(&chains)
                        .image_indices(&indices);
                    let result = if failed {
                        Err(vk::Result::ERROR_DEVICE_LOST)
                    } else {
                        unsafe { loader.queue_present(queue, &info) }
                    };
                    let _ = reply.send(result);
                    item.completion.finish(!failed);
                }
            }
        }
        if failed {
            // Pending resources cannot be reclaimed until the driver is drained, even on failure.
            unsafe {
                let _ = device.device_wait_idle();
            }
            for (_, completion) in pending.drain(..) {
                completion.finish(false);
            }
        } else {
            while let Some((fence, _)) = pending.front() {
                match unsafe { device.get_fence_status(*fence) } {
                    Ok(true) => {
                        pending.pop_front().unwrap().1.finish(true);
                    }
                    Ok(false) => break,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                }
            }
        }
        if disconnected && pending.is_empty() {
            for (_, item) in ready {
                item.completion.finish(false);
            }
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::Context;

    #[test]
    #[ignore = "requires a Vulkan device"]
    fn workers_record_in_parallel_but_retire_in_ticket_order() {
        let context = Context::new().expect("Vulkan device required");
        let (entered, entry) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let released = Mutex::new(released);
        let first: Operation = Arc::new(move |_, _| {
            entered.send(()).unwrap();
            released.lock().unwrap().recv().unwrap();
        });
        let first = context.command_workers.enqueue(vec![first], None, None);
        entry.recv_timeout(Duration::from_secs(5)).unwrap();
        let (recorded, recording) = mpsc::sync_channel(1);
        let second: Operation = Arc::new(move |_, _| {
            recorded.send(()).unwrap();
        });
        let second = context.command_workers.enqueue(vec![second], None, None);
        recording.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(second.0 .0.lock().unwrap().is_none());
        release.send(()).unwrap();
        first.wait().unwrap();
        second.wait().unwrap();
        // Reuse every worker's same pool after retirement.
        for _ in 0..32 {
            context
                .command_workers
                .enqueue(Vec::new(), None, None)
                .wait()
                .unwrap();
        }
    }

    #[test]
    #[ignore = "requires a Vulkan device"]
    fn failed_recording_unblocks_the_ordered_tail() {
        let context = Context::new().expect("Vulkan device required");
        let fail: Operation = Arc::new(|_, _| panic!("recording fixture"));
        let first = context.command_workers.enqueue(vec![fail], None, None);
        let second = context.command_workers.enqueue(Vec::new(), None, None);
        assert!(first.wait().is_none());
        assert!(second.wait().is_none());
        assert!(context.wait_idle().is_err());
    }
}
