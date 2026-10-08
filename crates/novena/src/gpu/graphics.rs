//! Bounded drawing with public Vulkan state and host choices. Provenance: 0027, 0028.
use super::{
    pipeline_workers::AsyncPipelines,
    pipelines::{
        stage_bindings, CacheStats, DriverCache, PersistenceStats, PipelineStatus, RequestError,
        TranslationIdentity,
    },
    Context, GlobalMemory,
};
use ash::vk;
use std::{collections::HashMap, path::Path, sync::Arc};

/// Opt-in host interpretation, not established guest enums. Provenance: 0028.
/// Viewport/scissor use Vulkan pixel coordinates. Raster state uses fill,
/// one sample and all color channels. Duplicate tokens are rejected at use.
#[derive(Clone, Debug)]
pub struct FirstDrawContract {
    pub topologies: Vec<(u32, PrimitiveTopology)>,
    pub attribute_formats: Vec<(u64, VertexFormat)>,
    /// Host-selected object spacing for counted bindings. None supports count one only.
    pub attribute_state_stride: Option<u64>,
    pub stream_state_stride: Option<u64>,
    pub cull_none: u64,
    pub rgba8: u64,
    pub target_2d: u64,
    pub identity_swizzle: [u64; 4],
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum PrimitiveTopology {
    TriangleList,
    TriangleStrip,
    TriangleFan,
}

impl PrimitiveTopology {
    fn vk(self) -> vk::PrimitiveTopology {
        match self {
            Self::TriangleList => vk::PrimitiveTopology::TRIANGLE_LIST,
            Self::TriangleStrip => vk::PrimitiveTopology::TRIANGLE_STRIP,
            Self::TriangleFan => vk::PrimitiveTopology::TRIANGLE_FAN,
        }
    }
}

/// Float-converting vertex formats from public Vulkan documentation, provenance 0028.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum VertexFormat {
    Float,
    Float2,
    Float3,
    Float4,
    Half2,
    Half4,
    Unorm8x4,
    Snorm8x4,
    Unorm16x2,
    Unorm16x4,
    Snorm16x2,
    Snorm16x4,
}

impl VertexFormat {
    pub(crate) fn vk(self) -> vk::Format {
        match self {
            Self::Float => vk::Format::R32_SFLOAT,
            Self::Float2 => vk::Format::R32G32_SFLOAT,
            Self::Float3 => vk::Format::R32G32B32_SFLOAT,
            Self::Float4 => vk::Format::R32G32B32A32_SFLOAT,
            Self::Half2 => vk::Format::R16G16_SFLOAT,
            Self::Half4 => vk::Format::R16G16B16A16_SFLOAT,
            Self::Unorm8x4 => vk::Format::R8G8B8A8_UNORM,
            Self::Snorm8x4 => vk::Format::R8G8B8A8_SNORM,
            Self::Unorm16x2 => vk::Format::R16G16_UNORM,
            Self::Unorm16x4 => vk::Format::R16G16B16A16_UNORM,
            Self::Snorm16x2 => vk::Format::R16G16_SNORM,
            Self::Snorm16x4 => vk::Format::R16G16B16A16_SNORM,
        }
    }

    pub(crate) fn layout(self) -> (u32, u32) {
        match self {
            Self::Float => (4, 4),
            Self::Float2 => (8, 4),
            Self::Float3 => (12, 4),
            Self::Float4 => (16, 4),
            Self::Half2 | Self::Unorm16x2 | Self::Snorm16x2 => (4, 2),
            Self::Half4 | Self::Unorm16x4 | Self::Snorm16x4 => (8, 2),
            Self::Unorm8x4 | Self::Snorm8x4 => (4, 1),
        }
    }
}

pub(crate) fn mapped<T: Copy, K: PartialEq>(table: &[(K, T)], token: K) -> Option<T> {
    let mut matches = table.iter().filter(|(key, _)| *key == token);
    let result = matches.next()?.1;
    matches.next().is_none().then_some(result)
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct VertexInput {
    pub bindings: Vec<VertexBinding>,
    pub attributes: Vec<VertexAttribute>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct VertexBinding {
    pub binding: u32,
    pub stride: u32,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct VertexAttribute {
    pub binding: u32,
    pub format: VertexFormat,
    pub offset: u32,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    vertex: Vec<u32>,
    fragment: Vec<u32>,
    input: VertexInput,
    topology: PrimitiveTopology,
}

pub(crate) struct GraphicsPipelines {
    driver: Arc<DriverCache>,
    pool: AsyncPipelines<Key, GraphicsPipeline>,
}
impl GraphicsPipelines {
    pub fn new(context: &Arc<Context>) -> Option<Self> {
        Self::build(context, None, 2, 64).ok()
    }
    pub fn persistent(
        context: &Arc<Context>,
        directory: &Path,
        identity: &TranslationIdentity,
        workers: usize,
        capacity: usize,
    ) -> Result<Self, String> {
        Self::build(context, Some((directory, identity)), workers, capacity)
    }
    fn build(
        context: &Arc<Context>,
        persistence: Option<(&Path, &TranslationIdentity)>,
        workers: usize,
        capacity: usize,
    ) -> Result<Self, String> {
        let driver = DriverCache::new(
            context,
            b"novena graphics main interface 1 disk 1",
            persistence,
        )?;
        let worker_driver = driver.clone();
        let pool = AsyncPipelines::new(
            HashMap::new(),
            CacheStats::default(),
            workers,
            capacity,
            move |key: &Key| {
                let pipeline = worker_driver
                    .create(|context, cache| GraphicsPipeline::create(context, cache, key))?;
                worker_driver.persist_driver();
                Ok(Arc::new(pipeline))
            },
        )?;
        Ok(Self { driver, pool })
    }
    /// A pending or queue-full request skips only this draw. Failures stay visible.
    pub fn request(
        &mut self,
        stages: &[Vec<u32>],
        input: VertexInput,
        topology: PrimitiveTopology,
    ) -> Result<Option<Arc<GraphicsPipeline>>, String> {
        let key = Key::new(stages, input, topology)?;
        let request = match self.pool.request(key) {
            Ok(request) => request,
            Err(RequestError::QueueFull) => return Ok(None),
            Err(RequestError::Stopped) => return Err("graphics compiler stopped".into()),
        };
        match request.poll() {
            PipelineStatus::Ready(pipeline) => Ok(Some(pipeline)),
            PipelineStatus::Failed(error) => Err(error),
            PipelineStatus::Queued | PipelineStatus::Compiling => Ok(None),
        }
    }
    pub fn retry_failed(
        &mut self,
        stages: &[Vec<u32>],
        input: VertexInput,
        topology: PrimitiveTopology,
    ) -> Result<bool, String> {
        Ok(self.pool.retry_failed(&Key::new(stages, input, topology)?))
    }
    pub fn stats(&self) -> CacheStats {
        self.pool.stats
    }
    pub fn pending(&self) -> usize {
        self.pool.pending()
    }
    pub fn persistence_stats(&self) -> PersistenceStats {
        self.driver.persistence_stats()
    }
    pub fn take_diagnostics(&self) -> Vec<String> {
        self.driver.take_diagnostics()
    }
}
impl Key {
    fn new(
        stages: &[Vec<u32>],
        input: VertexInput,
        topology: PrimitiveTopology,
    ) -> Result<Self, String> {
        if stages.len() != 2 {
            return Err("first draw requires exactly two translated stages".into());
        }
        let mut vertex = None;
        let mut fragment = None;
        for words in stages {
            let model = execution_model(words)?;
            let slot = match model {
                0 => &mut vertex,
                4 => &mut fragment,
                _ => return Err("unsupported graphics stage".into()),
            };
            if slot.replace(words).is_some() {
                return Err("duplicate graphics stage".into());
            }
        }
        let vertex = vertex.ok_or("missing vertex stage")?;
        let fragment = fragment.ok_or("missing fragment stage")?;
        Ok(Self {
            vertex: vertex.clone(),
            fragment: fragment.clone(),
            input,
            topology,
        })
    }
}

pub(crate) struct Draw {
    pub buffers: Vec<(u32, vk::Buffer, u64)>,
    pub viewport: vk::Viewport,
    pub scissor: vk::Rect2D,
    pub first: u32,
    pub count: u32,
}

fn instructions(words: &[u32]) -> Result<Vec<(u32, &[u32])>, String> {
    if words.len() < 5 || words[0] != 0x0723_0203 {
        return Err("invalid SPIR-V header".into());
    }
    let mut result = Vec::new();
    let mut at = 5;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        if count == 0 || count > words.len() - at {
            return Err("invalid SPIR-V instruction length".into());
        }
        result.push((words[at] & 0xffff, &words[at + 1..at + count]));
        at += count;
    }
    Ok(result)
}

fn execution_model(words: &[u32]) -> Result<u32, String> {
    let entries: Vec<_> = instructions(words)?
        .into_iter()
        .filter(|(op, _)| *op == 15)
        .collect();
    match entries.as_slice() {
        [(15, operands)] if operands.len() >= 4 => Ok(operands[0]),
        _ => Err("expected one graphics entry point".into()),
    }
}

// Inspect scalar/vector float interfaces from public types and decorations.
fn float_interface(words: &[u32], storage: u32) -> Result<Vec<(u32, u32)>, String> {
    let instructions = instructions(words)?;
    let mut types = HashMap::new();
    let mut locations = HashMap::new();
    let mut builtins = Vec::new();
    for &(op, args) in &instructions {
        match (op, args) {
            (19..=33, [id, ..]) => {
                types.insert(*id, (op, args));
            }
            (71, [id, 30, location]) => {
                locations.insert(*id, *location);
            }
            (71, [id, 11, _]) => builtins.push(*id),
            (72, [id, _, 11, _]) => builtins.push(*id),
            (71, [_, 31 | 32, _]) => {
                return Err("component/index interface decorations are unsupported".into())
            }
            _ => {}
        }
    }
    let mut result = Vec::new();
    for &(op, args) in &instructions {
        if op != 59 || args.len() < 3 || args[2] != storage {
            continue;
        }
        let Some(&(32, [_, actual, ty])) = types.get(&args[0]) else {
            return Err("invalid interface pointer".into());
        };
        if *actual != storage {
            return Err("interface storage mismatch".into());
        }
        if builtins.contains(&args[1]) || builtins.contains(ty) {
            if execution_model(words)? == 4 && storage == 3 {
                return Err("fragment builtin output is unsupported".into());
            }
            continue;
        }
        let (scalar, components) = match types.get(ty) {
            Some((23, [_, scalar, components @ 2..=4])) => (scalar, *components),
            Some((22, [_, 32])) => (ty, 1),
            _ => return Err("draw requires scalar/vector float interfaces".into()),
        };
        if !matches!(types.get(scalar), Some((22, [_, 32]))) {
            return Err("first draw requires 32-bit floats".into());
        }
        let location = *locations
            .get(&args[1])
            .ok_or("missing interface location")?;
        if result.iter().any(|&(at, _)| at == location) {
            return Err("duplicate interface location".into());
        }
        result.push((location, components));
    }
    result.sort_unstable();
    Ok(result)
}

pub(crate) struct GraphicsPipeline {
    context: Arc<Context>,
    pub render_pass: vk::RenderPass,
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
}

impl GraphicsPipeline {
    fn create(context: &Arc<Context>, cache: vk::PipelineCache, key: &Key) -> Result<Self, String> {
        if !stage_bindings(&key.vertex, 0)?.is_empty()
            || !stage_bindings(&key.fragment, 4)?.is_empty()
        {
            return Err("first draw does not support descriptors".into());
        }
        let inputs = float_interface(&key.vertex, 1)?;
        if inputs.is_empty()
            || inputs.len() != key.input.attributes.len()
            || inputs
                .iter()
                .enumerate()
                .any(|(i, &(at, _))| at != i as u32)
        {
            return Err("attribute state indices must match shader locations".into());
        }
        if float_interface(&key.vertex, 3)? != float_interface(&key.fragment, 1)? {
            return Err("vertex and fragment varying interfaces differ".into());
        }
        if float_interface(&key.fragment, 3)? != [(0, 4)] {
            return Err("draw requires float4 color output location zero".into());
        }
        let limits = unsafe {
            context
                .instance
                .get_physical_device_properties(context.physical_device)
        }
        .limits;
        if key.input.attributes.len() > limits.max_vertex_input_attributes as usize {
            return Err("too many vertex attributes".into());
        }
        for binding in &key.input.bindings {
            if binding.binding >= limits.max_vertex_input_bindings
                || binding.stride > limits.max_vertex_input_binding_stride
            {
                return Err("vertex binding exceeds device limits".into());
            }
        }
        for attribute in &key.input.attributes {
            if attribute.offset > limits.max_vertex_input_attribute_offset {
                return Err("vertex attribute exceeds device limits".into());
            }
            let supported = unsafe {
                context.instance.get_physical_device_format_properties(
                    context.physical_device,
                    attribute.format.vk(),
                )
            };
            if !supported
                .buffer_features
                .contains(vk::FormatFeatureFlags::VERTEX_BUFFER)
            {
                return Err("vertex format is unavailable".into());
            }
        }
        let mut result = Self {
            context: context.clone(),
            render_pass: vk::RenderPass::null(),
            pipeline: vk::Pipeline::null(),
            layout: vk::PipelineLayout::null(),
        };
        // SAFETY: these are fixed public Vulkan states, with owned partial handles.
        unsafe {
            let attachments = [vk::AttachmentDescription::default()
                .format(vk::Format::R8G8B8A8_UNORM)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::LOAD)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let colors = [vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
            let subpasses = [vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(&colors)];
            result.render_pass = context
                .device
                .create_render_pass(
                    &vk::RenderPassCreateInfo::default()
                        .attachments(&attachments)
                        .subpasses(&subpasses),
                    None,
                )
                .map_err(|e| format!("create draw render pass: {e:?}"))?;
            let ranges = [GlobalMemory::push_constant_range(
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            )];
            result.layout = context
                .device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&ranges),
                    None,
                )
                .map_err(|e| format!("create draw layout: {e:?}"))?;
        }
        let vertex = context.create_global_shader_module(&key.vertex)?;
        let fragment = match context.create_global_shader_module(&key.fragment) {
            Ok(module) => module,
            Err(error) => {
                unsafe {
                    context.device.destroy_shader_module(vertex, None);
                }
                return Err(error);
            }
        };
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vertex)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fragment)
                .name(c"main"),
        ];
        let bindings: Vec<_> = key
            .input
            .bindings
            .iter()
            .map(|b| {
                vk::VertexInputBindingDescription::default()
                    .binding(b.binding)
                    .stride(b.stride)
                    .input_rate(vk::VertexInputRate::VERTEX)
            })
            .collect();
        let attributes: Vec<_> = key
            .input
            .attributes
            .iter()
            .enumerate()
            .map(|(i, a)| {
                vk::VertexInputAttributeDescription::default()
                    .binding(a.binding)
                    .location(i as u32)
                    .format(a.format.vk())
                    .offset(a.offset)
            })
            .collect();
        let input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&bindings)
            .vertex_attribute_descriptions(&attributes);
        let assembly =
            vk::PipelineInputAssemblyStateCreateInfo::default().topology(key.topology.vk());
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let samples = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let depth = vk::PipelineDepthStencilStateCreateInfo::default();
        let colors = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&colors);
        let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamics = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&samples)
            .depth_stencil_state(&depth)
            .color_blend_state(&blend)
            .dynamic_state(&dynamics)
            .layout(result.layout)
            .render_pass(result.render_pass);
        let created = unsafe {
            context
                .device
                .create_graphics_pipelines(cache, &[info], None)
        };
        unsafe {
            context.device.destroy_shader_module(vertex, None);
            context.device.destroy_shader_module(fragment, None);
        }
        match created {
            Ok(pipelines) => result.pipeline = pipelines[0],
            Err((pipelines, error)) => {
                unsafe {
                    for pipeline in pipelines {
                        context.device.destroy_pipeline(pipeline, None);
                    }
                }
                return Err(format!("create graphics pipeline: {error:?}"));
            }
        }
        Ok(result)
    }

    /// Caller owns the active compatible render pass, live arena slice and completion.
    pub unsafe fn record(&self, command: vk::CommandBuffer, memory: &GlobalMemory, draw: &Draw) {
        let device = &self.context.device;
        device.cmd_bind_pipeline(command, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
        for &(binding, buffer, offset) in &draw.buffers {
            device.cmd_bind_vertex_buffers(command, binding, &[buffer], &[offset]);
        }
        device.cmd_set_viewport(command, 0, &[draw.viewport]);
        device.cmd_set_scissor(command, 0, &[draw.scissor]);
        memory.push_delta(
            command,
            self.layout,
            vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
        );
        device.cmd_draw(command, draw.count, 1, draw.first, 0);
    }
}

impl Drop for GraphicsPipeline {
    fn drop(&mut self) {
        unsafe {
            self.context.device.destroy_pipeline(self.pipeline, None);
            self.context
                .device
                .destroy_pipeline_layout(self.layout, None);
            self.context
                .device
                .destroy_render_pass(self.render_pass, None);
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
    fn interface(model: u32, storage: u32, location: u32) -> Vec<u32> {
        let mut words = vec![0x0723_0203, 0x0001_0300, 0, 100, 0];
        instruction(&mut words, 15, &[model, 90, 0x6e69_616d, 0]);
        instruction(&mut words, 22, &[1, 32]);
        instruction(&mut words, 23, &[2, 1, 4]);
        instruction(&mut words, 32, &[3, storage, 2]);
        instruction(&mut words, 59, &[3, 4, storage]);
        instruction(&mut words, 71, &[4, 30, location]);
        words
    }
    #[test]
    fn derives_stage_and_single_float4_location_from_translation() {
        let vertex = interface(0, 1, 3);
        assert_eq!(execution_model(&vertex).unwrap(), 0);
        assert_eq!(float_interface(&vertex, 1).unwrap(), [(3, 4)]);
        assert!(float_interface(&vertex, 3).unwrap().is_empty());
        assert!(stage_bindings(&vertex, 0).unwrap().is_empty());
        assert!(stage_bindings(&vertex, 4).is_err());
        let fragment = interface(4, 3, 0);
        assert_eq!(execution_model(&fragment).unwrap(), 4);
        assert_eq!(float_interface(&fragment, 3).unwrap(), [(0, 4)]);
    }
    #[test]
    fn accepts_extra_float_attributes_and_rejects_bad_interfaces() {
        let mut words = interface(0, 1, 0);
        instruction(&mut words, 59, &[3, 5, 1]);
        instruction(&mut words, 71, &[5, 30, 1]);
        assert_eq!(float_interface(&words, 1).unwrap(), [(0, 4), (1, 4)]);
        instruction(&mut words, 71, &[5, 30, 0]);
        assert!(float_interface(&words, 1)
            .unwrap_err()
            .contains("duplicate"));
        let mut words = interface(0, 1, 0);
        words[12] = 16; // Float width.
        assert!(float_interface(&words, 1).is_err());
        let mut words = interface(4, 3, 0);
        instruction(&mut words, 71, &[4, 11, 22]); // FragDepth.
        assert!(float_interface(&words, 3).unwrap_err().contains("builtin"));
        let mut words = interface(0, 1, 0);
        words.push(0);
        assert!(execution_model(&words).unwrap_err().contains("length"));
    }
}
