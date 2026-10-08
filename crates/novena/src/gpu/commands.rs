//! Bounded command reuse with a fence for every slot. Provenance: 0026.

use super::Context;
use ash::vk;
use std::sync::Arc;

const FRAMES: usize = 2;

struct Frame {
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    acquire: vk::Semaphore,
}

pub(super) struct Commands {
    context: Arc<Context>,
    frames: Vec<Frame>,
    next: usize,
    failed: bool,
}

impl Commands {
    pub fn new(context: &Arc<Context>) -> Option<Self> {
        let mut commands = Self {
            context: Arc::clone(context),
            frames: Vec::new(),
            next: 0,
            failed: false,
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

    pub fn begin(&mut self) -> Option<(vk::CommandBuffer, vk::Semaphore)> {
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
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )
                .ok()?;
        }
        Some((frame.command, frame.acquire))
    }

    pub fn submit(&mut self, wait: bool, signal: Option<vk::Semaphore>) -> Option<()> {
        let frame = &self.frames[self.next];
        let device = &self.context.device;
        let buffers = [frame.command];
        let waits = [frame.acquire];
        let stages = [vk::PipelineStageFlags::TRANSFER];
        let signals: Vec<_> = signal.into_iter().collect();
        let mut submit = vk::SubmitInfo::default()
            .command_buffers(&buffers)
            .signal_semaphores(&signals);
        if wait {
            submit = submit.wait_semaphores(&waits).wait_dst_stage_mask(&stages);
        }
        unsafe {
            device.end_command_buffer(frame.command).ok()?;
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
        self.next = (self.next + 1) % self.frames.len();
        Some(())
    }

    pub fn wait(&self) -> Option<()> {
        if self.failed {
            return None;
        }
        let fences: Vec<_> = self.frames.iter().map(|frame| frame.fence).collect();
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
