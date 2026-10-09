//! Bounded command reuse with a fence for every slot. Provenance: 0026.

use super::Context;
use ash::vk;
use std::sync::Arc;

const FRAMES: usize = 2;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct TransferKey {
    /// Image handle included in the cached transfer identity.
    pub image: u64,
    /// Scratch buffer handle included in the cached transfer identity.
    pub scratch: u64,
    /// Arena buffer device address for this transfer.
    pub address: u64,
    /// Scratch buffer device address for this transfer.
    pub scratch_address: u64,
    /// Tracked Vulkan image layout at recording time.
    pub layout: i32,
    /// True copies arena storage to the image; false copies it back.
    pub load: bool,
    pub conversion: bool,
    /// Checked byte packing for the image storage.
    pub packing: crate::tiling::Layout,
}

struct Frame {
    acquire: vk::Semaphore,
    completion: Option<super::command_workers::Completion>,
    cached: Option<(Vec<TransferKey>, Vec<super::recording_device::Operation>)>,
}
pub(super) struct Commands {
    context: Arc<Context>,
    frames: Vec<Frame>,
    next: usize,
    capture: Option<super::recording_device::Capture>,
    replay: Option<Vec<super::recording_device::Operation>>,
    pending: Option<Vec<TransferKey>>,
}
impl Commands {
    pub fn new(context: &Arc<Context>) -> Option<Self> {
        let mut commands = Self {
            context: context.clone(),
            frames: Vec::new(),
            next: 0,
            capture: None,
            replay: None,
            pending: None,
        };
        for _ in 0..FRAMES {
            let acquire = unsafe {
                context
                    .device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                    .ok()?
            };
            commands.frames.push(Frame {
                acquire,
                completion: None,
                cached: None,
            });
        }
        Some(commands)
    }
    pub fn slot(&self) -> usize {
        self.next
    }
    pub fn slots(&self) -> usize {
        self.frames.len()
    }
    pub fn invalidate_cached(&mut self) {
        for frame in &mut self.frames {
            frame.cached = None;
        }
        self.pending = None;
        self.replay = None;
    }
    pub fn begin(&mut self) -> Option<(vk::CommandBuffer, vk::Semaphore)> {
        self.capture.take();
        self.replay = None;
        self.pending = None;
        let frame = &mut self.frames[self.next];
        if let Some(completion) = frame.completion.take() {
            completion.wait()?;
        }
        frame.cached = None;
        let capture = super::recording_device::Capture::new();
        let command = capture.command();
        self.capture = Some(capture);
        Some((command, frame.acquire))
    }
    pub fn begin_cached(&mut self, key: Vec<TransferKey>) -> Option<(vk::CommandBuffer, bool)> {
        let frame = &mut self.frames[self.next];
        if let Some(completion) = frame.completion.take() {
            completion.wait()?;
        }
        if let Some((cached, operations)) = &frame.cached {
            if *cached == key {
                self.capture.take();
                self.pending = None;
                self.replay = Some(operations.clone());
                return Some((vk::CommandBuffer::null(), true));
            }
        }
        let (command, _) = self.begin()?;
        self.pending = Some(key);
        Some((command, false))
    }
    pub fn submit(&mut self, wait: bool, signal: Option<vk::Semaphore>) -> Option<()> {
        let operations = if let Some(operations) = self.replay.take() {
            operations
        } else {
            self.capture.take()?.finish()
        };
        let frame = &mut self.frames[self.next];
        if let Some(key) = self.pending.take() {
            frame.cached = Some((key, operations.clone()));
        }
        frame.completion = Some(self.context.command_workers.enqueue(
            operations,
            wait.then_some(frame.acquire),
            signal,
        ));
        self.next = (self.next + 1) % self.frames.len();
        Some(())
    }
    pub fn wait(&self) -> Option<()> {
        for frame in &self.frames {
            if let Some(completion) = &frame.completion {
                completion.wait()?;
            }
        }
        Some(())
    }
}
impl Drop for Commands {
    fn drop(&mut self) {
        self.capture.take();
        let _ = self.context.wait_idle();
        for frame in &self.frames {
            unsafe {
                self.context.device.destroy_semaphore(frame.acquire, None);
            }
        }
    }
}
