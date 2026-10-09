//! Bounded command reuse with timeline completion values. Provenance: 0034.

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
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    completion: u64,
    acquire: vk::Semaphore,
    cached: Option<Vec<TransferKey>>,
}

pub(super) struct Commands {
    context: Arc<Context>,
    frames: Vec<Frame>,
    next: usize,
    timeline: vk::Semaphore,
    submitted: u64,
    failed: bool,
    reusing: bool,
    pending: Option<Vec<TransferKey>>,
}

impl Commands {
    /// Create reusable execution resources; return None if Vulkan setup fails.
    pub fn new(context: &Arc<Context>) -> Option<Self> {
        Self::with_capacity(context, FRAMES)
    }

    pub fn with_capacity(context: &Arc<Context>, capacity: usize) -> Option<Self> {
        let mut commands = Self {
            context: Arc::clone(context),
            frames: Vec::new(),
            next: 0,
            timeline: vk::Semaphore::null(),
            submitted: 0,
            failed: false,
            reusing: false,
            pending: None,
        };
        let mut ty =
            vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
        commands.timeline = unsafe {
            context
                .device
                .create_semaphore(&vk::SemaphoreCreateInfo::default().push_next(&mut ty), None)
                .ok()?
        };
        for _ in 0..capacity {
            let device = &context.device;
            // Own each partially created frame before the next fallible call.
            let pool = unsafe {
                device
                    .create_command_pool(
                        &vk::CommandPoolCreateInfo::default()
                            .queue_family_index(context.queue_family),
                        None,
                    )
                    .ok()?
            };
            commands.frames.push(Frame {
                pool,
                command: vk::CommandBuffer::null(),
                completion: 0,
                acquire: vk::Semaphore::null(),
                cached: None,
            });
            let frame = commands.frames.last_mut()?;
            frame.command = unsafe {
                device
                    .allocate_command_buffers(
                        &vk::CommandBufferAllocateInfo::default()
                            .command_pool(pool)
                            .level(vk::CommandBufferLevel::PRIMARY)
                            .command_buffer_count(1),
                    )
                    .ok()?[0]
            };
            frame.acquire = unsafe {
                device
                    .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
                    .ok()?
            };
        }
        Some(commands)
    }

    pub fn slot(&self) -> usize {
        self.next
    }

    pub fn slots(&self) -> usize {
        self.frames.len()
    }

    /// Wait and discard cached recordings before their resources change.
    pub fn invalidate_cached(&mut self) {
        for frame in &mut self.frames {
            frame.cached = None;
        }
        self.pending = None;
        self.reusing = false;
    }

    /// Begin a fresh command recording and return its reusable completion semaphore.
    pub fn begin(&mut self) -> Option<(vk::CommandBuffer, vk::Semaphore)> {
        self.begin_with_flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
    }

    /// Select a transfer recording by complete identity; report whether it already exists.
    pub fn begin_cached(&mut self, key: Vec<TransferKey>) -> Option<(vk::CommandBuffer, bool)> {
        if self.failed {
            return None;
        }
        let frame = &self.frames[self.next];
        if frame.cached.as_ref() == Some(&key) {
            self.wait_value(frame.completion)?;
            self.reusing = true;
            self.pending = None;
            return Some((frame.command, true));
        }
        let (command, _) = self.begin_with_flags(vk::CommandBufferUsageFlags::empty())?;
        self.pending = Some(key);
        Some((command, false))
    }

    fn begin_with_flags(
        &mut self,
        flags: vk::CommandBufferUsageFlags,
    ) -> Option<(vk::CommandBuffer, vk::Semaphore)> {
        self.reusing = false;
        self.pending = None;
        self.frames[self.next].cached = None;
        if self.failed {
            return None;
        }
        let frame = &self.frames[self.next];
        let device = &self.context.device;
        self.wait_value(frame.completion)?;
        unsafe {
            device
                .reset_command_pool(frame.pool, vk::CommandPoolResetFlags::empty())
                .ok()?;
            device
                .begin_command_buffer(
                    frame.command,
                    &vk::CommandBufferBeginInfo::default().flags(flags),
                )
                .ok()?;
        }
        Some((frame.command, frame.acquire))
    }

    /// Submit the selected command; optionally wait and signal the supplied semaphore.
    pub fn submit(&mut self, wait: bool, signal: Option<vk::Semaphore>) -> Option<()> {
        let frame = &self.frames[self.next];
        let device = &self.context.device;
        let buffers = [frame.command];
        let waits = [frame.acquire];
        let stages = [vk::PipelineStageFlags::TRANSFER];
        let value = self.submitted.checked_add(1)?;
        let signals = [self.timeline, signal.unwrap_or(vk::Semaphore::null())];
        let values = [value, 0];
        let count = 1 + usize::from(signal.is_some());
        let wait_values = [0];
        let mut timeline =
            vk::TimelineSemaphoreSubmitInfo::default().signal_semaphore_values(&values[..count]);
        if wait {
            timeline = timeline.wait_semaphore_values(&wait_values);
        }
        let mut submit = vk::SubmitInfo::default()
            .command_buffers(&buffers)
            .signal_semaphores(&signals[..count])
            .push_next(&mut timeline);
        if wait {
            submit = submit.wait_semaphores(&waits).wait_dst_stage_mask(&stages);
        }
        unsafe {
            if !self.reusing {
                device.end_command_buffer(frame.command).ok()?;
            }
            if device
                .queue_submit(self.context.queue, &[submit], vk::Fence::null())
                .is_err()
            {
                // A failed submission has no completion value to wait for.
                self.failed = true;
                return None;
            }
        }
        self.submitted = value;
        self.frames[self.next].completion = value;
        if let Some(key) = self.pending.take() {
            self.frames[self.next].cached = Some(key);
        }
        self.next = (self.next + 1) % self.frames.len();
        Some(())
    }

    /// Wait for retained command submissions to complete; return None on failure.
    pub fn wait(&self) -> Option<()> {
        self.wait_value(self.submitted)
    }

    pub fn completion(&self) -> u64 {
        self.submitted
    }

    pub fn ready(&self, value: u64) -> Option<bool> {
        unsafe {
            self.context
                .device
                .get_semaphore_counter_value(self.timeline)
                .ok()
                .map(|v| v >= value)
        }
    }

    pub fn wait_value(&self, value: u64) -> Option<()> {
        if self.failed {
            return None;
        }
        if value == 0 {
            return Some(());
        }
        let semaphores = [self.timeline];
        let values = [value];
        unsafe {
            self.context
                .device
                .wait_semaphores(
                    &vk::SemaphoreWaitInfo::default()
                        .semaphores(&semaphores)
                        .values(&values),
                    u64::MAX,
                )
                .ok()
        }
    }
}

impl Drop for Commands {
    fn drop(&mut self) {
        let _ = self.wait();
        unsafe {
            let device = &self.context.device;
            device.destroy_semaphore(self.timeline, None);
            for frame in &self.frames {
                device.destroy_semaphore(frame.acquire, None);
                device.destroy_command_pool(frame.pool, None);
            }
        }
    }
}
