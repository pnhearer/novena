//! Content-keyed compute pipelines. Evidence: provenance 0025.
use super::{Context, GlobalMemory};
use crate::{ShaderTranslator, SHADER_STAGE_UNKNOWN};
use ash::vk;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

/// One resource declaration in the translated SPIR-V, including descriptor arrays.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DescriptorBinding {
    pub set: u32,
    pub binding: u32,
    pub descriptor_type: vk::DescriptorType,
    pub count: u32,
}

impl std::fmt::Debug for DescriptorBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DescriptorBinding")
            .field("set", &self.set)
            .field("binding", &self.binding)
            .field("descriptor_type", &self.descriptor_type.as_raw())
            .field("count", &self.count)
            .finish()
    }
}

#[derive(Default)]
struct Decorations {
    set: Option<u32>,
    binding: Option<u32>,
    block: bool,
    buffer_block: bool,
}

/// Reflect the supported compute interface. This does not replace SPIR-V validation.
/// Runtime descriptor arrays, decoration groups and additional push constants
/// require a later executor contract.
pub fn compute_bindings(words: &[u32]) -> Result<Vec<DescriptorBinding>, String> {
    if words.len() < 5 || words[0] != 0x0723_0203 {
        return Err("invalid SPIR-V header".into());
    }
    let mut types = HashMap::new();
    let mut constants = HashMap::new();
    let mut decorations = HashMap::<u32, Decorations>::new();
    let mut offsets = HashMap::new();
    let mut variables = Vec::new();
    let mut entries = 0;
    let mut at = 5;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        let opcode = words[at] & 0xffff;
        if count == 0 || count > words.len() - at {
            return Err("invalid SPIR-V instruction length".into());
        }
        let operands = &words[at + 1..at + count];
        match opcode {
            15 => {
                let name: Vec<_> = operands
                    .get(2..)
                    .unwrap_or_default()
                    .iter()
                    .flat_map(|word| word.to_le_bytes())
                    .take_while(|&byte| byte != 0)
                    .collect();
                if operands.first() != Some(&5) || name != b"main" {
                    return Err("expected compute entry point main".into());
                }
                entries += 1;
            }
            19..=33 if !operands.is_empty() => {
                types.insert(operands[0], (opcode, operands));
            }
            43 if operands.len() == 3 => {
                constants.insert(operands[1], (operands[0], operands[2]));
            }
            59 if operands.len() >= 3 => {
                variables.push((operands[0], operands[1], operands[2]));
            }
            71 if operands.len() >= 2 => {
                let decoration = decorations.entry(operands[0]).or_default();
                match operands[1..] {
                    [2] => decoration.block = true,
                    [3] => decoration.buffer_block = true,
                    [kind @ (33 | 34), value] => {
                        let slot = if kind == 33 {
                            &mut decoration.binding
                        } else {
                            &mut decoration.set
                        };
                        if slot.replace(value).is_some() {
                            return Err("duplicate descriptor decoration".into());
                        }
                    }
                    _ => {}
                }
            }
            72 if operands.len() == 4 && operands[2] == 35 => {
                offsets.insert((operands[0], operands[1]), operands[3]);
            }
            73..=75 => return Err("decoration groups are unsupported".into()),
            _ => {}
        }
        at += count;
    }
    if entries != 1 {
        return Err("expected one compute entry point".into());
    }
    let node = |id: u32| {
        types
            .get(&id)
            .copied()
            .ok_or_else(|| "missing SPIR-V type".to_owned())
    };
    let mut bindings = BTreeMap::new();
    let mut push_blocks = 0;
    for (pointer, variable, storage) in variables {
        if !matches!(storage, 0 | 2 | 9 | 12) {
            continue;
        }
        let (opcode, pointer) = node(pointer)?;
        if opcode != 32 || pointer.len() != 3 || pointer[1] != storage {
            return Err("invalid resource pointer".into());
        }
        let mut type_id = pointer[2];
        if storage == 9 {
            push_blocks += 1;
            let (opcode, structure) = node(type_id)?;
            if opcode != 30
                || structure.len() != 2
                || offsets.get(&(type_id, 0)) != Some(&0)
                || !decorations.get(&type_id).is_some_and(|d| d.block)
            {
                return Err("expected only the global delta push constant".into());
            }
            let (opcode, integer) = node(structure[1])?;
            if opcode != 21 || integer.get(1..) != Some(&[64, 0][..]) {
                return Err("global delta must be uint64".into());
            }
            continue;
        }
        let decoration = decorations
            .get(&variable)
            .ok_or("missing descriptor decorations")?;
        let set = decoration.set.ok_or("missing descriptor set")?;
        let binding = decoration.binding.ok_or("missing descriptor binding")?;
        let mut count = 1_u32;
        let mut depth = 0;
        while node(type_id)?.0 == 28 {
            depth += 1;
            if depth > types.len() {
                return Err("cyclic descriptor type".into());
            }
            let (_, array) = node(type_id)?;
            if array.len() != 3 {
                return Err("invalid descriptor array".into());
            }
            let &(integer, length) = constants
                .get(&array[2])
                .ok_or("nonconstant descriptor count")?;
            let (opcode, integer) = node(integer)?;
            if opcode != 21
                || integer.get(1) != Some(&32)
                || integer.len() != 3
                || !matches!(integer[2], 0 | 1)
                || length == 0
                || (integer[2] == 1 && length > i32::MAX as u32)
            {
                return Err("unsupported descriptor count".into());
            }
            count = count
                .checked_mul(length)
                .ok_or("descriptor count overflow")?;
            type_id = array[1];
        }
        let (opcode, ty) = node(type_id)?;
        let descriptor_type = match (storage, opcode) {
            (0, 26) => vk::DescriptorType::SAMPLER,
            (0, 25) if ty.len() >= 8 && matches!(ty[2], 0..=3) => match ty[6] {
                1 => vk::DescriptorType::SAMPLED_IMAGE,
                2 => vk::DescriptorType::STORAGE_IMAGE,
                _ => return Err("unknown image descriptor type".into()),
            },
            (2, 30) if decorations.get(&type_id).is_some_and(|d| d.block) => {
                vk::DescriptorType::UNIFORM_BUFFER
            }
            (2, 30) if decorations.get(&type_id).is_some_and(|d| d.buffer_block) => {
                vk::DescriptorType::STORAGE_BUFFER
            }
            (12, 30) if decorations.get(&type_id).is_some_and(|d| d.block) => {
                vk::DescriptorType::STORAGE_BUFFER
            }
            _ => return Err("unsupported descriptor type".into()),
        };
        let descriptor = DescriptorBinding {
            set,
            binding,
            descriptor_type,
            count,
        };
        if bindings.insert((set, binding), descriptor).is_some() {
            return Err("duplicate descriptor binding".into());
        }
    }
    if push_blocks > 1 {
        return Err("multiple push constant blocks".into());
    }
    Ok(bindings.into_values().collect())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
}

/// A device-local cache with one fixed translator and translation configuration.
/// The translator must produce stable output for a byte sequence during this cache's life.
pub struct ComputePipelines {
    context: Arc<Context>,
    translator: Arc<dyn ShaderTranslator>,
    pipelines: HashMap<Vec<u8>, Arc<ComputePipeline>>,
    stats: CacheStats,
    driver_cache: vk::PipelineCache,
}

impl ComputePipelines {
    pub fn new(
        context: &Arc<Context>,
        translator: Arc<dyn ShaderTranslator>,
    ) -> Result<Self, String> {
        // SAFETY: the context owns the device; initial cache data is empty.
        let driver_cache = unsafe {
            context
                .device
                .create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None)
        }
        .map_err(|error| format!("create pipeline cache: {error:?}"))?;
        Ok(Self {
            context: context.clone(),
            translator,
            pipelines: HashMap::new(),
            stats: CacheStats::default(),
            driver_cache,
        })
    }

    /// The translator must supply SPIR-V valid for this device, including features
    /// beyond the narrow global-memory feature checks. A miss uses its emitted
    /// specialization defaults.
    ///
    /// Header plus code, as supplied by Novena's shader record reader.
    /// Full byte equality handles hash collisions. Guest addresses are absent.
    pub fn get_or_compile(&mut self, program: &[u8]) -> Result<Arc<ComputePipeline>, String> {
        if let Some(pipeline) = self.pipelines.get(program) {
            self.stats.hits += 1;
            return Ok(pipeline.clone());
        }
        self.stats.misses += 1;
        let words = self.translator.translate(SHADER_STAGE_UNKNOWN, program)?;
        let pipeline = Arc::new(ComputePipeline::create(
            &self.context,
            self.driver_cache,
            &words,
        )?);
        self.pipelines.insert(program.to_vec(), pipeline.clone());
        Ok(pipeline)
    }

    pub fn stats(&self) -> CacheStats {
        self.stats
    }
}

impl Drop for ComputePipelines {
    fn drop(&mut self) {
        // SAFETY: pipeline creation borrows this cache mutably; no call is pending.
        unsafe {
            self.context
                .device
                .destroy_pipeline_cache(self.driver_cache, None);
        }
    }
}

/// Owns the pipeline and all its layouts independently of the content cache.
pub struct ComputePipeline {
    context: Arc<Context>,
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    set_layouts: Vec<vk::DescriptorSetLayout>,
    bindings: Vec<DescriptorBinding>,
}

impl ComputePipeline {
    fn create(
        context: &Arc<Context>,
        cache: vk::PipelineCache,
        words: &[u32],
    ) -> Result<Self, String> {
        let bindings = compute_bindings(words)?;
        let mut result = Self {
            context: context.clone(),
            pipeline: vk::Pipeline::null(),
            layout: vk::PipelineLayout::null(),
            set_layouts: Vec::new(),
            bindings,
        };
        // SAFETY: context is live; property queries have no mutable device state.
        let limits = unsafe {
            context
                .instance
                .get_physical_device_properties(context.physical_device)
        }
        .limits;
        let sets = match result.bindings.last() {
            Some(binding) => binding
                .set
                .checked_add(1)
                .ok_or("descriptor set overflow")?,
            None => 0,
        };
        if sets > limits.max_bound_descriptor_sets {
            return Err("descriptor set limit exceeded".into());
        }
        let mut total = 0_u32;
        for (kind, per_stage, all_sets) in [
            (
                vk::DescriptorType::SAMPLER,
                limits.max_per_stage_descriptor_samplers,
                limits.max_descriptor_set_samplers,
            ),
            (
                vk::DescriptorType::SAMPLED_IMAGE,
                limits.max_per_stage_descriptor_sampled_images,
                limits.max_descriptor_set_sampled_images,
            ),
            (
                vk::DescriptorType::STORAGE_IMAGE,
                limits.max_per_stage_descriptor_storage_images,
                limits.max_descriptor_set_storage_images,
            ),
            (
                vk::DescriptorType::UNIFORM_BUFFER,
                limits.max_per_stage_descriptor_uniform_buffers,
                limits.max_descriptor_set_uniform_buffers,
            ),
            (
                vk::DescriptorType::STORAGE_BUFFER,
                limits.max_per_stage_descriptor_storage_buffers,
                limits.max_descriptor_set_storage_buffers,
            ),
        ] {
            let count = result
                .bindings
                .iter()
                .filter(|b| b.descriptor_type == kind)
                .try_fold(0_u32, |count, b| count.checked_add(b.count))
                .ok_or("descriptor count overflow")?;
            if count > per_stage || count > all_sets {
                return Err("descriptor resource limit exceeded".into());
            }
            // Vulkan excludes standalone samplers from maxPerStageResources.
            if kind != vk::DescriptorType::SAMPLER {
                total = total
                    .checked_add(count)
                    .ok_or("descriptor count overflow")?;
            }
        }
        if total > limits.max_per_stage_resources {
            return Err("per-stage resource limit exceeded".into());
        }
        // SAFETY: layouts are built from supported reflected declarations.
        unsafe {
            for set in 0..sets {
                let bindings: Vec<_> = result
                    .bindings
                    .iter()
                    .filter(|b| b.set == set)
                    .map(|b| {
                        vk::DescriptorSetLayoutBinding::default()
                            .binding(b.binding)
                            .descriptor_type(b.descriptor_type)
                            .descriptor_count(b.count)
                            .stage_flags(vk::ShaderStageFlags::COMPUTE)
                    })
                    .collect();
                let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
                let mut support = vk::DescriptorSetLayoutSupport::default();
                context
                    .device
                    .get_descriptor_set_layout_support(&info, &mut support);
                if support.supported == vk::FALSE {
                    return Err("unsupported descriptor set layout".into());
                }
                result.set_layouts.push(
                    context
                        .device
                        .create_descriptor_set_layout(&info, None)
                        .map_err(|error| format!("create descriptor layout: {error:?}"))?,
                );
            }
            let ranges = [GlobalMemory::push_constant_range(
                vk::ShaderStageFlags::COMPUTE,
            )];
            result.layout = context
                .device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&result.set_layouts)
                        .push_constant_ranges(&ranges),
                    None,
                )
                .map_err(|error| format!("create pipeline layout: {error:?}"))?;
        }
        let module = context.create_global_shader_module(words)?;
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(module)
            .name(c"main");
        // SAFETY: the temporary module, cache and layout belong to this device.
        let created = unsafe {
            context.device.create_compute_pipelines(
                cache,
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(stage)
                    .layout(result.layout)],
                None,
            )
        };
        // SAFETY: Vulkan has finished reading the module, including on failure.
        unsafe {
            context.device.destroy_shader_module(module, None);
        }
        match created {
            Ok(pipelines) => result.pipeline = pipelines[0],
            Err((pipelines, error)) => {
                // SAFETY: failed batch creation may return partial owned pipelines.
                unsafe {
                    for pipeline in pipelines {
                        context.device.destroy_pipeline(pipeline, None);
                    }
                }
                return Err(format!("create compute pipeline: {error:?}"));
            }
        }
        Ok(result)
    }

    pub fn bindings(&self) -> &[DescriptorBinding] {
        &self.bindings
    }
    pub fn set_layouts(&self) -> &[vk::DescriptorSetLayout] {
        &self.set_layouts
    }

    /// Bind, push the arena delta, dispatch, and make shader writes visible to host readback.
    ///
    /// # Safety
    /// The command must be recording outside a render pass on this context's compute queue family.
    /// Descriptor sets must match these layouts, be fully populated, and reference live resources
    /// with the required image layouts. The caller owns submission synchronization and keeps this
    /// pipeline, the arena, descriptor sets and resources alive until commands finish.
    /// Synchronize command-buffer access and all dependencies on prior GPU/host work.
    /// The host-read barrier does not supply dependencies between successive dispatches.
    pub unsafe fn record_dispatch(
        &self,
        command: vk::CommandBuffer,
        memory: &GlobalMemory,
        sets: &[vk::DescriptorSet],
        groups: [u32; 3],
    ) -> Result<(), String> {
        if self.context.device.handle() != memory.context().device.handle() {
            return Err("arena belongs to another device".into());
        }
        if sets.len() != self.set_layouts.len() {
            return Err("descriptor set count does not match pipeline".into());
        }
        let limits = self
            .context
            .instance
            .get_physical_device_properties(self.context.physical_device)
            .limits;
        if groups
            .iter()
            .zip(limits.max_compute_work_group_count)
            .any(|(&groups, limit)| groups > limit)
        {
            return Err("workgroup count limit exceeded".into());
        }
        let device = &self.context.device;
        device.cmd_bind_pipeline(command, vk::PipelineBindPoint::COMPUTE, self.pipeline);
        if !sets.is_empty() {
            device.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::COMPUTE,
                self.layout,
                0,
                sets,
                &[],
            );
        }
        memory.push_delta(command, self.layout, vk::ShaderStageFlags::COMPUTE);
        device.cmd_dispatch(command, groups[0], groups[1], groups[2]);
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::HOST,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ)],
            &[],
            &[],
        );
        Ok(())
    }
}

impl Drop for ComputePipeline {
    fn drop(&mut self) {
        // SAFETY: record_dispatch requires the caller to retain us until completion.
        // On creation failure the null handles and partial layouts are owned here too.
        unsafe {
            self.context.device.destroy_pipeline(self.pipeline, None);
            self.context
                .device
                .destroy_pipeline_layout(self.layout, None);
            for &layout in &self.set_layouts {
                self.context
                    .device
                    .destroy_descriptor_set_layout(layout, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instruction(words: &mut Vec<u32>, opcode: u32, operands: &[u32]) {
        words.push(((operands.len() as u32 + 1) << 16) | opcode);
        words.extend_from_slice(operands);
    }

    fn interface() -> Vec<u32> {
        let mut words = vec![0x0723_0203, 0x0001_0300, 0, 100, 0];
        instruction(&mut words, 15, &[5, 90, 0x6e69_616d, 0]);
        instruction(&mut words, 21, &[1, 32, 0]);
        instruction(&mut words, 43, &[1, 2, 3]);
        instruction(&mut words, 22, &[3, 32]);
        instruction(&mut words, 25, &[4, 3, 1, 0, 0, 0, 1, 0]);
        instruction(&mut words, 26, &[5]);
        instruction(&mut words, 25, &[6, 3, 1, 0, 0, 0, 2, 33]);
        instruction(&mut words, 30, &[7, 1]);
        instruction(&mut words, 71, &[7, 2]);
        instruction(&mut words, 29, &[8, 1]);
        instruction(&mut words, 30, &[9, 8]);
        instruction(&mut words, 71, &[9, 2]);
        instruction(&mut words, 28, &[10, 7, 2]);
        // UBO array, separate texture/sampler, storage image, storage buffer
        // with a runtime member array. Set 1 is intentionally absent.
        for (id, ty, storage, set, binding) in [
            (20, 10, 2, 0, 3),
            (22, 4, 0, 0, 12),
            (24, 5, 0, 0, 13),
            (26, 6, 0, 2, 2048),
            (28, 9, 12, 2, 5),
        ] {
            instruction(&mut words, 32, &[id, storage, ty]);
            instruction(&mut words, 59, &[id, id + 1, storage]);
            instruction(&mut words, 71, &[id + 1, 34, set]);
            instruction(&mut words, 71, &[id + 1, 33, binding]);
        }
        words
    }

    #[test]
    fn reflects_resource_types_counts_and_sparse_binding_numbers() {
        let bindings = compute_bindings(&interface()).unwrap();
        assert_eq!(bindings.len(), 5);
        for (binding, (set, number, ty, count)) in bindings.iter().zip([
            (0, 3, vk::DescriptorType::UNIFORM_BUFFER, 3),
            (0, 12, vk::DescriptorType::SAMPLED_IMAGE, 1),
            (0, 13, vk::DescriptorType::SAMPLER, 1),
            (2, 5, vk::DescriptorType::STORAGE_BUFFER, 1),
            (2, 2048, vk::DescriptorType::STORAGE_IMAGE, 1),
        ]) {
            assert_eq!(
                (binding.set, binding.binding, binding.count),
                (set, number, count)
            );
            assert_eq!(binding.descriptor_type.as_raw(), ty.as_raw());
        }
    }

    #[test]
    fn rejects_runtime_descriptor_arrays_and_conflicting_bindings() {
        let mut words = interface();
        // Replace fixed UBO descriptor array with a runtime array.
        let at = words
            .windows(4)
            .position(|w| w == [(4 << 16) | 28, 10, 7, 2])
            .unwrap();
        words.splice(at..at + 4, [(3 << 16) | 29, 10, 7]);
        assert!(compute_bindings(&words)
            .unwrap_err()
            .contains("unsupported descriptor type"));
        let mut words = interface();
        instruction(&mut words, 59, &[20, 40, 2]);
        instruction(&mut words, 71, &[40, 34, 0]);
        instruction(&mut words, 71, &[40, 33, 3]);
        assert!(compute_bindings(&words)
            .unwrap_err()
            .contains("duplicate descriptor"));
    }

    #[test]
    fn recognizes_only_the_published_global_delta_push_block() {
        let mut words = interface();
        instruction(&mut words, 21, &[50, 64, 0]);
        instruction(&mut words, 30, &[51, 50]);
        instruction(&mut words, 71, &[51, 2]);
        instruction(&mut words, 72, &[51, 0, 35, 0]);
        instruction(&mut words, 32, &[52, 9, 51]);
        instruction(&mut words, 59, &[52, 53, 9]);
        assert!(compute_bindings(&words).is_ok());
        // Extra members would need a different pipeline layout contract.
        let at = words
            .windows(3)
            .position(|w| w == [(3 << 16) | 30, 51, 50])
            .unwrap();
        words.splice(at..at + 3, [(4 << 16) | 30, 51, 50, 1]);
        assert!(compute_bindings(&words)
            .unwrap_err()
            .contains("only the global delta"));
    }

    #[test]
    fn rejects_truncated_instructions_and_graphics_entry_points() {
        let mut words = interface();
        words.push(4 << 16);
        assert!(compute_bindings(&words)
            .unwrap_err()
            .contains("instruction length"));
        let mut words = interface();
        words[6] = 0; // Vertex execution model.
        assert!(compute_bindings(&words)
            .unwrap_err()
            .contains("compute entry point"));
    }
}
