//! Bounded execution of recorded first draws. Evidence: signatures 0009, provenance 0027.
use super::objects::{Object, RecordedCommand, ShaderTranslation, StateCommand};
use crate::{
    gpu::{graphics::Draw, Backend},
    Instance, Status,
};
use ash::vk;
use std::collections::HashMap;

type Settings = Vec<(&'static str, [u64; 6])>;

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
    buffer: Option<(u64, u64, u64)>,
    states: HashMap<&'static str, Option<Settings>>,
    viewport: Option<[u64; 5]>,
    scissor: Option<[u64; 5]>,
    unsupported: bool,
}

impl State {
    pub fn record(&mut self, command: RecordedCommand) {
        match command {
            RecordedCommand::BindProgram(program) => self.program = Some(program),
            RecordedCommand::State(StateCommand::BindVertexBuffer {
                index: stream,
                address,
                size,
            }) => self.buffer = Some((stream, address, size)),
            RecordedCommand::BindState { kind, settings }
            | RecordedCommand::State(StateCommand::BindState { kind, settings, .. })
                if matches!(
                    kind,
                    "VertexStreamState"
                        | "VertexAttribState"
                        | "ColorState"
                        | "DepthStencilState"
                        | "PolygonState"
                ) =>
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
        let contract = backend.first_draw.ok_or(Status::Unimplemented)?;
        if self.unsupported
            || targets.len() != 1
            || depth != 0
            || views != [0, 0]
            || primitive != contract.triangle_list
        {
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
        let stream = self.settings("VertexStreamState", &["Stride", "Divisor"])?;
        let stride = value(stream, "Stride")?[0];
        if value(stream, "Divisor")?[0] != 0 {
            return Err(Status::Unimplemented);
        }
        let attribute = self.settings("VertexAttribState", &["Format", "StreamIndex"])?;
        let format = value(attribute, "Format")?;
        if format[0] != contract.float4 || value(attribute, "StreamIndex")?[0] != 0 {
            return Err(Status::Unimplemented);
        }
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
        let stride = u32::try_from(stride).map_err(|_| Status::BadArgument)?;
        let offset = u32::try_from(format[1]).map_err(|_| Status::BadArgument)?;
        if stride == 0 || !stride.is_multiple_of(4) || !offset.is_multiple_of(4) {
            return Err(Status::BadArgument);
        }
        let (stream, address, size) = self.buffer.ok_or(Status::Unimplemented)?;
        if stream != 0 {
            return Err(Status::Unimplemented);
        }
        let resolution = instance
            .objects
            .resolve_gpu_address(address)
            .map_err(|_| Status::BadArgument)?;
        if size == 0 || size > resolution.remaining {
            return Err(Status::BadArgument);
        }
        let end = vertex_end(first, count, stride, offset).ok_or(Status::BadArgument)?;
        if end > size {
            return Err(Status::BadArgument);
        }
        let memory = backend.global_memory.as_ref().ok_or(Status::BadArgument)?;
        let (buffer, at) = memory
            .image_region(
                resolution.pool,
                resolution.offset,
                usize::try_from(size).map_err(|_| Status::BadArgument)?,
            )
            .ok_or(Status::BadArgument)?;
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
        let Some(pipeline) = backend
            .graphics
            .request(&stages, stride, offset)
            .map_err(|_| Status::Unimplemented)?
        else {
            return Ok(());
        };
        if !backend.ensure_texture(targets[0], &description, false) {
            return Err(Status::BadArgument);
        }
        let draw = Draw {
            buffer,
            offset: at,
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

fn vertex_end(first: u32, count: u32, stride: u32, offset: u32) -> Option<u64> {
    if count == 0 {
        return Some(0);
    }
    let last = first.checked_add(count - 1)?;
    u64::from(last)
        .checked_mul(u64::from(stride))?
        .checked_add(u64::from(offset))?
        .checked_add(16)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vertex_bounds_include_first_offset_and_last_attribute() {
        assert_eq!(vertex_end(2, 3, 32, 8), Some(152));
        assert_eq!(vertex_end(u32::MAX, 2, 16, 0), None);
        assert_eq!(vertex_end(u32::MAX, 0, 16, 0), Some(0));
    }
}
