//! Bounded execution of recorded draws. Evidence: signatures 0009, provenance 0027, 0028, 0029.
use super::objects::{Object, RecordedCommand, ShaderTranslation, StateCommand, StateSettings};
use crate::{
    gpu::{
        graphics::{execution_model, mapped, Draw, VertexAttribute, VertexBinding, VertexInput},
        uniforms::{self, UniformStage, BANK_SIZE},
        Backend,
    },
    Instance, Status,
};
use ash::vk;
use std::collections::HashMap;

type Settings = StateSettings;

pub(super) struct Request<'a> {
    pub targets: &'a [u64],
    pub depth: u64,
    pub views: [u64; 2],
    pub primitive: u32,
    pub first: u32,
    pub count: u32,
}

#[derive(Default)]
pub(super) struct State {
    program: Option<u64>,
    buffers: HashMap<u64, (u64, u64)>,
    vertex_states: HashMap<&'static str, Option<Vec<Settings>>>,
    uniforms: HashMap<(u64, u64), (u64, u64)>,
    states: HashMap<&'static str, Option<Settings>>,
    viewport: Option<[u64; 5]>,
    scissor: Option<[u64; 5]>,
    unsupported: bool,
}

impl State {
    pub fn record(&mut self, command: RecordedCommand) {
        match command {
            RecordedCommand::State(StateCommand::BindUniformBuffer {
                stage,
                index,
                address,
                size,
            }) => {
                self.uniforms.insert((stage, index), (address, size));
            }
            RecordedCommand::BindProgram(program) => self.program = Some(program),
            RecordedCommand::State(StateCommand::BindVertexBuffer {
                index: stream,
                address,
                size,
            }) => {
                self.buffers.insert(stream, (address, size));
            }
            RecordedCommand::BindVertexStates { kind, settings } => {
                self.vertex_states.insert(kind, settings);
            }
            RecordedCommand::BindState { kind, settings }
            | RecordedCommand::State(StateCommand::BindState { kind, settings, .. })
                if matches!(kind, "ColorState" | "DepthStencilState" | "PolygonState") =>
            {
                self.states.insert(kind, settings);
            }
            RecordedCommand::SetViewport(viewport) => self.viewport = Some(viewport),
            RecordedCommand::SetScissor(scissor) => self.scissor = Some(scissor),
            RecordedCommand::SetDepthRange(range) => {
                // The experiment fixes the Vulkan depth interval to [0, 1].
                if range != [0, 0, u64::from(1.0_f32.to_bits())] {
                    self.unsupported = true;
                }
            }
            _ => self.unsupported = true,
        }
    }

    fn settings(&self, kind: &str, allowed: &[&str]) -> Result<&Settings, Status> {
        let settings = self
            .states
            .get(kind)
            .and_then(Option::as_ref)
            .ok_or(Status::Unimplemented)?;
        if settings.iter().any(|(name, _)| !allowed.contains(name)) {
            return Err(Status::Unimplemented);
        }
        Ok(settings)
    }

    pub fn execute(
        &self,
        instance: &Instance,
        backend: &mut Backend,
        request: Request<'_>,
    ) -> Result<(), Status> {
        let Request {
            targets,
            depth,
            views,
            primitive,
            first,
            count,
        } = request;
        let contract = backend.first_draw.as_ref().ok_or(Status::Unimplemented)?;
        let topology = mapped(&contract.topologies, primitive).ok_or(Status::Unimplemented)?;
        if self.unsupported || targets.len() != 1 || depth != 0 || views != [0, 0] {
            return Err(Status::Unimplemented);
        }
        let Some(Object::Texture { description, .. }) = instance.objects.get(targets[0]) else {
            return Err(Status::BadArgument);
        };
        if description.pool == 0
            || description.format != contract.rgba8
            || description.depth > 1
            || description.levels > 1
            || description.target != contract.target_2d
            || description.swizzle != contract.identity_swizzle
            || description.flags != 0
            || description.depth_stencil_mode != 0
        {
            return Err(Status::Unimplemented);
        }
        let streams = self.vertex_settings("VertexStreamState", &["Stride", "Divisor"])?;
        let attributes = self.vertex_settings("VertexAttribState", &["Format", "StreamIndex"])?;
        let color = self.settings("ColorState", &["BlendEnable"])?;
        if color.iter().any(|(_, args)| args[0] != 0) || value(color, "BlendEnable")?[1] != 0 {
            return Err(Status::Unimplemented);
        }
        let depth_state = self.settings(
            "DepthStencilState",
            &["DepthTestEnable", "DepthWriteEnable", "StencilTestEnable"],
        )?;
        for name in ["DepthTestEnable", "DepthWriteEnable", "StencilTestEnable"] {
            if value(depth_state, name)?[0] != 0 {
                return Err(Status::Unimplemented);
            }
        }
        let polygon = self.settings("PolygonState", &["CullFace"])?;
        if value(polygon, "CullFace")?[0] != contract.cull_none {
            return Err(Status::Unimplemented);
        }
        let memory = backend.global_memory.as_ref().ok_or(Status::BadArgument)?;
        let mut input = VertexInput {
            bindings: Vec::new(),
            attributes: Vec::new(),
        };
        let mut buffers = Vec::new();
        for attribute in attributes {
            let format = value(attribute, "Format")?;
            let format_kind =
                mapped(&contract.attribute_formats, format[0]).ok_or(Status::Unimplemented)?;
            let offset = u32::try_from(format[1]).map_err(|_| Status::BadArgument)?;
            let binding = u32::try_from(value(attribute, "StreamIndex")?[0])
                .map_err(|_| Status::BadArgument)?;
            let stream = streams.get(binding as usize).ok_or(Status::Unimplemented)?;
            if value(stream, "Divisor")?[0] != 0 {
                return Err(Status::Unimplemented);
            }
            let stride =
                u32::try_from(value(stream, "Stride")?[0]).map_err(|_| Status::BadArgument)?;
            let (bytes, alignment) = format_kind.layout();
            let (address, size) = *self
                .buffers
                .get(&u64::from(binding))
                .ok_or(Status::Unimplemented)?;
            let resolution = instance
                .objects
                .resolve_gpu_address(address)
                .map_err(|_| Status::BadArgument)?;
            // Every fetched element, including the arena binding address, must be aligned.
            let (buffer, at) = memory
                .image_region(
                    resolution.pool,
                    resolution.offset,
                    usize::try_from(size).map_err(|_| Status::BadArgument)?,
                )
                .ok_or(Status::BadArgument)?;
            if size == 0
                || size > resolution.remaining
                || !stride.is_multiple_of(alignment)
                || !(at + u64::from(offset)).is_multiple_of(u64::from(alignment))
                || vertex_end(first, count, stride, offset, bytes).ok_or(Status::BadArgument)?
                    > size
            {
                return Err(Status::BadArgument);
            }
            input.attributes.push(VertexAttribute {
                binding,
                format: format_kind,
                offset,
            });
            if !input.bindings.iter().any(|b| b.binding == binding) {
                input.bindings.push(VertexBinding { binding, stride });
                buffers.push((binding, buffer, at));
            }
        }
        input.bindings.sort_unstable_by_key(|b| b.binding);
        buffers.sort_unstable_by_key(|b| b.0);
        let viewport = rectangle(self.viewport, description.width, description.height)?;
        let scissor = rectangle(self.scissor, description.width, description.height)?;
        let stages = match instance
            .objects
            .get(self.program.ok_or(Status::Unimplemented)?)
        {
            Some(Object::Program {
                shader_translations,
                ..
            }) => shader_translations
                .into_iter()
                .map(|translation| match translation {
                    ShaderTranslation::Spirv(words) => Ok(words),
                    ShaderTranslation::Error(_) => Err(Status::Unimplemented),
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(Status::BadArgument),
        };
        let context = backend.context().clone();
        let limits = unsafe {
            context
                .instance
                .get_physical_device_properties(context.physical_device)
        }
        .limits;
        if description.width > u64::from(limits.max_framebuffer_width)
            || description.height > u64::from(limits.max_framebuffer_height)
            || viewport
                .iter()
                .zip(limits.max_viewport_dimensions)
                .any(|(&size, limit)| size > limit)
            || limits.viewport_bounds_range[0] > 0.0
            || viewport
                .iter()
                .any(|&size| size as f32 > limits.viewport_bounds_range[1])
        {
            return Err(Status::BadArgument);
        }
        if self.uniforms.keys().any(|&(stage, index)| {
            !backend
                .uniforms
                .bindings
                .iter()
                .any(|m| m.stage == stage && m.index == index)
        }) {
            return Err(Status::Unimplemented);
        }
        let storage = backend
            .graphics
            .uses_storage(backend.uniforms.storage_buffers);
        let mut uniform_buffers = Vec::new();
        for stage in [UniformStage::Vertex, UniformStage::Fragment] {
            let words = stages
                .iter()
                .find(|words| execution_model(words).ok() == Some(stage.model()))
                .ok_or(Status::Unimplemented)?;
            for bank in uniforms::banks(words, stage).map_err(|_| Status::Unimplemented)? {
                let mapping = backend
                    .uniforms
                    .bindings
                    .iter()
                    .find(|m| m.target == stage && m.bank == bank.bank)
                    .ok_or(Status::Unimplemented)?;
                let &(address, size) = self
                    .uniforms
                    .get(&(mapping.stage, mapping.index))
                    .ok_or(Status::Unimplemented)?;
                let resolution = instance
                    .objects
                    .resolve_gpu_address(address)
                    .map_err(|_| Status::BadArgument)?;
                if size == 0
                    || size > BANK_SIZE
                    || !size.is_multiple_of(16)
                    || size > resolution.remaining
                {
                    return Err(Status::BadArgument);
                }
                let info = if storage {
                    memory.storage_buffer_info(resolution.pool, resolution.offset, size)
                } else {
                    memory.uniform_buffer_info(resolution.pool, resolution.offset, size)
                }
                .ok_or(Status::BadArgument)?;
                uniform_buffers.push(info);
            }
        }
        let Some(pipeline) = backend
            .graphics
            .request(&stages, input, topology, storage)
            .map_err(|_| Status::Unimplemented)?
        else {
            return Ok(());
        };
        if !backend.ensure_texture(targets[0], &description, false) {
            return Err(Status::BadArgument);
        }
        let descriptors = pipeline
            .descriptors(&uniform_buffers)
            .map_err(|_| Status::InternalError)?;
        let draw = Draw {
            descriptors,
            buffers,
            first,
            count,
            viewport: vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: viewport[0] as f32,
                height: viewport[1] as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            },
            scissor: vk::Rect2D::default().extent(vk::Extent2D {
                width: scissor[0],
                height: scissor[1],
            }),
        };
        backend
            .draw(targets[0], &pipeline, &draw)
            .ok_or(Status::InternalError)
    }

    fn vertex_settings(&self, kind: &str, allowed: &[&str]) -> Result<&Vec<Settings>, Status> {
        let settings = self
            .vertex_states
            .get(kind)
            .and_then(Option::as_ref)
            .ok_or(Status::Unimplemented)?;
        if settings
            .iter()
            .flatten()
            .any(|(name, _)| !allowed.contains(name))
        {
            return Err(Status::Unimplemented);
        }
        Ok(settings)
    }
}

fn value(settings: &Settings, name: &str) -> Result<[u64; 6], Status> {
    settings
        .iter()
        .rev()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
        .ok_or(Status::Unimplemented)
}

fn rectangle(recorded: Option<[u64; 5]>, width: u64, height: u64) -> Result<[u32; 2], Status> {
    let [_, x, y, w, h] = recorded.ok_or(Status::Unimplemented)?;
    if x != 0 || y != 0 {
        return Err(Status::Unimplemented);
    }
    if w == 0 || h == 0 || w > width || h > height {
        return Err(Status::BadArgument);
    }
    Ok([
        u32::try_from(w).map_err(|_| Status::BadArgument)?,
        u32::try_from(h).map_err(|_| Status::BadArgument)?,
    ])
}

fn vertex_end(first: u32, count: u32, stride: u32, offset: u32, bytes: u32) -> Option<u64> {
    if count == 0 {
        return Some(0);
    }
    let last = first.checked_add(count - 1)?;
    u64::from(last)
        .checked_mul(u64::from(stride))?
        .checked_add(u64::from(offset))?
        .checked_add(u64::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vertex_bounds_include_first_offset_and_last_attribute() {
        assert_eq!(vertex_end(2, 3, 32, 8, 16), Some(152));
        assert_eq!(vertex_end(2, 3, 32, 8, 4), Some(140));
        assert_eq!(vertex_end(u32::MAX, 2, 16, 0, 16), None);
        assert_eq!(vertex_end(u32::MAX, 0, 16, 0, 16), Some(0));
        assert_eq!(vertex_end(10, 3, 0, 2, 4), Some(6));
    }
}
