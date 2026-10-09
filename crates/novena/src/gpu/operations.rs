//! Explicit command hypotheses and synchronized arena operations. Provenance: 0037.
use super::{commands::Commands, pipelines::ComputePipeline, Context, GlobalMemory};
use ash::vk;
use std::{collections::HashMap, sync::Arc};

/// Host interpretation of a recorded buffer binding for compute.
#[derive(Clone, Debug)]
pub struct ComputeBufferMapping {
    /// Recorded binding index at the selected compute stage.
    pub index: u64,
    /// Reflected descriptor set.
    pub set: u32,
    /// Reflected descriptor binding.
    pub binding: u32,
}

/// Decode an unresolved whole-texture clear into a key, color and channel mask.
pub type ClearDecoder = Arc<dyn Fn([u64; 7]) -> Option<(u64, [f32; 4], u32)> + Send + Sync>;

/// Opt-in hypotheses. Tokens and record layouts are not recovered interface facts.
#[derive(Clone)]
pub struct CommandContract {
    /// Counter token selecting accumulated passing samples.
    pub occlusion: u64,
    /// Counter token selecting a device timestamp.
    pub timestamp: u64,
    /// Predicate mode selecting a nonzero 32-bit arena word.
    pub condition_nonzero: u64,
    /// Predicate mode selecting a zero 32-bit arena word.
    pub condition_zero: u64,
    /// Recorded stage token selecting compute buffer bindings.
    pub compute_stage: u64,
    /// Explicit binding-to-descriptor mappings.
    pub compute_buffers: Vec<ComputeBufferMapping>,
    /// Unresolved clear record decoder, absent by default.
    pub clear: Option<ClearDecoder>,
}
impl CommandContract {
    pub(crate) fn valid(&self) -> bool {
        self.occlusion != self.timestamp
            && self.condition_nonzero != self.condition_zero
            && self.compute_buffers.iter().enumerate().all(|(i, a)| {
                self.compute_buffers[..i]
                    .iter()
                    .all(|b| a.index != b.index && (a.set, a.binding) != (b.set, b.binding))
            })
    }
}

/// Instance range and optional Vulkan indirect argument buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub instances: u32,
    pub first_instance: u32,
    pub indirect: Option<(vk::Buffer, u64)>,
    pub index_skip: u64,
}
impl Default for Geometry {
    fn default() -> Self {
        Self {
            instances: 1,
            first_instance: 0,
            indirect: None,
            index_skip: 0,
        }
    }
}

pub(crate) struct Execution {
    context: Arc<Context>,
    commands: Commands,
    timestamp: vk::QueryPool,
    occlusion: vk::QueryPool,
    counting: bool,
    samples: u64,
    pipelines: HashMap<Vec<u32>, Arc<ComputePipeline>>,
}
impl Execution {
    pub fn new(context: &Arc<Context>) -> Option<Self> {
        Some(Self {
            context: context.clone(),
            commands: Commands::new(context)?,
            timestamp: vk::QueryPool::null(),
            occlusion: vk::QueryPool::null(),
            counting: false,
            samples: 0,
            pipelines: HashMap::new(),
        })
    }

    fn query(&self, ty: vk::QueryType) -> Option<vk::QueryPool> {
        unsafe {
            self.context
                .device
                .create_query_pool(
                    &vk::QueryPoolCreateInfo::default()
                        .query_type(ty)
                        .query_count(1),
                    None,
                )
                .ok()
        }
    }

    pub fn reset_occlusion(&mut self) -> Option<()> {
        if self.occlusion == vk::QueryPool::null() {
            self.occlusion = self.query(vk::QueryType::OCCLUSION)?;
        }
        self.samples = 0;
        self.counting = true;
        Some(())
    }

    pub fn occlusion_query(&self) -> Option<vk::QueryPool> {
        self.counting.then_some(self.occlusion)
    }

    pub fn collect_occlusion(&mut self) -> Option<()> {
        if self.counting {
            let mut result = [0_u64];
            unsafe {
                self.context
                    .device
                    .get_query_pool_results(
                        self.occlusion,
                        0,
                        &mut result,
                        vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
                    )
                    .ok()?;
            }
            self.samples = self.samples.checked_add(result[0])?;
        }
        Some(())
    }

    /// Full memory dependency at the boundary of each synchronous operation.
    unsafe fn barrier(&self, command: vk::CommandBuffer) {
        self.context.device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
            vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE | vk::AccessFlags::HOST_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::MEMORY_READ
                        | vk::AccessFlags::MEMORY_WRITE
                        | vk::AccessFlags::HOST_READ,
                )],
            &[],
            &[],
        );
    }

    fn complete(&mut self, command: vk::CommandBuffer) -> Option<()> {
        unsafe {
            self.barrier(command);
        }
        self.commands.submit(false, None)?;
        self.commands.wait()
    }

    pub fn fill(&mut self, region: (vk::Buffer, u64), size: u64, value: u32) -> Option<()> {
        if !region.1.is_multiple_of(4) || !size.is_multiple_of(4) || size == 0 {
            return None;
        }
        let (command, _) = self.commands.begin()?;
        unsafe {
            self.barrier(command);
            self.context
                .device
                .cmd_fill_buffer(command, region.0, region.1, size, value);
        }
        self.complete(command)
    }

    pub fn copy(
        &mut self,
        source: (vk::Buffer, u64),
        destination: (vk::Buffer, u64),
        size: u64,
    ) -> Option<()> {
        if size == 0
            || (source.0 == destination.0
                && source.1 < destination.1.checked_add(size)?
                && destination.1 < source.1.checked_add(size)?)
        {
            return None;
        }
        let (command, _) = self.commands.begin()?;
        unsafe {
            self.barrier(command);
            self.context.device.cmd_copy_buffer(
                command,
                source.0,
                destination.0,
                &[vk::BufferCopy::default()
                    .src_offset(source.1)
                    .dst_offset(destination.1)
                    .size(size)],
            );
        }
        self.complete(command)
    }

    pub fn report(
        &mut self,
        memory: &mut GlobalMemory,
        pool: u64,
        offset: u64,
        samples: bool,
    ) -> Option<()> {
        let (buffer, at) = memory.buffer_region(pool, offset, 16)?;
        if !at.is_multiple_of(8) {
            return None;
        }
        let families = unsafe {
            self.context
                .instance
                .get_physical_device_queue_family_properties(self.context.physical_device)
        };
        if families
            .get(self.context.queue_family as usize)?
            .timestamp_valid_bits
            == 0
        {
            return None;
        }
        if self.timestamp == vk::QueryPool::null() {
            self.timestamp = self.query(vk::QueryType::TIMESTAMP)?;
        }
        let value = if samples { self.samples } else { 0 };
        memory.write_pool(pool, offset, &value.to_le_bytes())?;
        let (command, _) = self.commands.begin()?;
        unsafe {
            self.barrier(command);
            let device = &self.context.device;
            device.cmd_reset_query_pool(command, self.timestamp, 0, 1);
            device.cmd_write_timestamp(
                command,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                self.timestamp,
                0,
            );
            device.cmd_copy_query_pool_results(
                command,
                self.timestamp,
                0,
                1,
                buffer,
                at + 8,
                8,
                vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
            );
        }
        self.complete(command)
    }

    pub fn dispatch(
        &mut self,
        memory: &GlobalMemory,
        words: &[u32],
        resources: &[(u32, u32, vk::DescriptorBufferInfo)],
        groups: [u32; 3],
        indirect: Option<(vk::Buffer, u64)>,
    ) -> Option<()> {
        let pipeline = if let Some(p) = self.pipelines.get(words) {
            p.clone()
        } else {
            let p = Arc::new(
                ComputePipeline::create(&self.context, vk::PipelineCache::null(), words).ok()?,
            );
            self.pipelines.insert(words.to_vec(), p.clone());
            p
        };
        let bindings = pipeline.bindings();
        if bindings.len() != resources.len()
            || bindings.iter().any(|b| {
                b.count != 1
                    || !matches!(
                        b.descriptor_type,
                        vk::DescriptorType::UNIFORM_BUFFER | vk::DescriptorType::STORAGE_BUFFER
                    )
                    || !resources.iter().any(|r| (r.0, r.1) == (b.set, b.binding))
            })
        {
            return None;
        }
        let sizes: Vec<_> = bindings
            .iter()
            .map(|b| {
                vk::DescriptorPoolSize::default()
                    .ty(b.descriptor_type)
                    .descriptor_count(1)
            })
            .collect();
        let context = self.context.clone();
        let device = &context.device;
        let layouts = pipeline.set_layouts();
        let pool = if layouts.is_empty() {
            vk::DescriptorPool::null()
        } else {
            unsafe {
                device
                    .create_descriptor_pool(
                        &vk::DescriptorPoolCreateInfo::default()
                            .max_sets(layouts.len() as u32)
                            .pool_sizes(&sizes),
                        None,
                    )
                    .ok()?
            }
        };
        let result = (|| {
            let sets = if layouts.is_empty() {
                Vec::new()
            } else {
                unsafe {
                    device
                        .allocate_descriptor_sets(
                            &vk::DescriptorSetAllocateInfo::default()
                                .descriptor_pool(pool)
                                .set_layouts(layouts),
                        )
                        .ok()?
                }
            };
            for b in bindings {
                let r = resources
                    .iter()
                    .find(|r| (r.0, r.1) == (b.set, b.binding))?;
                unsafe {
                    device.update_descriptor_sets(
                        &[vk::WriteDescriptorSet::default()
                            .dst_set(sets[b.set as usize])
                            .dst_binding(b.binding)
                            .descriptor_type(b.descriptor_type)
                            .buffer_info(std::slice::from_ref(&r.2))],
                        &[],
                    );
                }
            }
            let (command, _) = self.commands.begin()?;
            unsafe {
                self.barrier(command);
                if let Some((buffer, offset)) = indirect {
                    pipeline
                        .record_dispatch_indirect(command, memory, &sets, groups, buffer, offset)
                        .ok()?;
                } else {
                    pipeline
                        .record_dispatch(command, memory, &sets, groups)
                        .ok()?;
                }
            }
            self.complete(command)
        })();
        unsafe {
            if result.is_none() {
                let _ = device.device_wait_idle();
            }
            device.destroy_descriptor_pool(pool, None);
        }
        result
    }
}
impl Drop for Execution {
    fn drop(&mut self) {
        unsafe {
            let _ = self.context.device.device_wait_idle();
            self.context.device.destroy_query_pool(self.timestamp, None);
            self.context.device.destroy_query_pool(self.occlusion, None);
        }
    }
}
