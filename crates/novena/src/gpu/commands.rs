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
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    acquire: vk::Semaphore,
    cached: Option<Vec<TransferKey>>,
}

pub(super) struct Commands {
    context: Arc<Context>,
    frames: Vec<Frame>,
    next: usize,
    failed: bool,
    reusing: bool,
    pending: Option<Vec<TransferKey>>,
}

impl Commands {
    /// Create reusable execution resources; return None if Vulkan setup fails.
    pub fn new(context: &Arc<Context>) -> Option<Self> {
        let mut commands = Self {
            context: Arc::clone(context),
            frames: Vec::new(),
            next: 0,
            failed: false,
            reusing: false,
            pending: None,
        };
        for _ in 0..FRAMES {
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
                fence: vk::Fence::null(),
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
            frame.fence = unsafe {
                device
                    .create_fence(
                        &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                        None,
                    )
                    .ok()?
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
            unsafe {
                self.context
                    .device
                    .wait_for_fences(&[frame.fence], true, u64::MAX)
                    .ok()?;
            }
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
        unsafe {
            device
                .wait_for_fences(&[frame.fence], true, u64::MAX)
                .ok()?;
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
        let signals = [signal.unwrap_or(vk::Semaphore::null())];
        let mut submit = vk::SubmitInfo::default()
            .command_buffers(&buffers)
            .signal_semaphores(&signals[..usize::from(signal.is_some())]);
        if wait {
            submit = submit.wait_semaphores(&waits).wait_dst_stage_mask(&stages);
        }
        unsafe {
            if !self.reusing {
                device.end_command_buffer(frame.command).ok()?;
            }
            device.reset_fences(&[frame.fence]).ok()?;
            if device
                .queue_submit(self.context.queue, &[submit], frame.fence)
                .is_err()
            {
                // This fence will never signal. Prevent a later indefinite wait.
                self.failed = true;
                return None;
            }
        }
        if let Some(key) = self.pending.take() {
            self.frames[self.next].cached = Some(key);
        }
        self.next = (self.next + 1) % self.frames.len();
        Some(())
    }

    /// Wait for retained command submissions to complete; return None on failure.
    pub fn wait(&self) -> Option<()> {
        if self.failed {
            return None;
        }
        let fences: [_; FRAMES] = std::array::from_fn(|i| self.frames[i].fence);
        unsafe {
            self.context
                .device
                .wait_for_fences(&fences, true, u64::MAX)
                .ok()
        }
    }
}

impl Drop for Commands {
    fn drop(&mut self) {
        unsafe {
            let device = &self.context.device;
            let _ = device.device_wait_idle();
            for frame in &self.frames {
                device.destroy_semaphore(frame.acquire, None);
                device.destroy_fence(frame.fence, None);
                device.destroy_command_pool(frame.pool, None);
            }
        }
    }
}
