use super::{
    objects::{Object, RecordedCommand},
    read_f32x4, read_u64, Handler,
};
use crate::instance::{Instance, Registers, Status};

fn record(
    instance: &Instance,
    function: u32,
    registers: &mut Registers,
    command: RecordedCommand,
) -> Status {
    instance.objects.update(registers.x[0], |object| {
        if let Object::CommandBuffer {
            recording: true,
            commands,
            ..
        } = object
        {
            commands.push(command);
        }
    });
    let _ = function;
    // Recorded commands return nothing novena knows of. The result
    // registers are cleared, as for an unhandled call, so a caller that does
    // read a result gets zero rather than its own first argument back.
    registers.x[0] = 0;
    registers.x[1] = 0;
    registers.d[0] = 0;
    Status::Ok
}

pub fn handler(name: &str) -> Option<Handler> {
    if !name.starts_with("nvnCommandBuffer") {
        return None;
    }
    Some(match name {
        "nvnCommandBufferSetRenderTargets" => |instance, _, r| {
            let count = r.x[1].min(16);
            let mut colors = Vec::with_capacity(count as usize);
            for i in 0..count {
                let Some(value) = read_u64(instance, r.x[2] + i * 8) else {
                    return Status::BadArgument;
                };
                colors.push(value);
            }
            record(
                instance,
                0,
                r,
                RecordedCommand::SetRenderTargets {
                    colors,
                    depth: r.x[4],
                },
            )
        },
        "nvnCommandBufferClearColor" => |instance, _, r| {
            let Some(color) = read_f32x4(instance, r.x[2]) else {
                return Status::BadArgument;
            };
            record(
                instance,
                0,
                r,
                RecordedCommand::ClearColor {
                    index: r.x[1] as u32,
                    color,
                    mask: r.x[3] as u32,
                },
            )
        },
        "nvnCommandBufferClearDepthStencil" => |instance, _, r| {
            record(
                instance,
                0,
                r,
                RecordedCommand::ClearDepthStencil {
                    depth: f32::from_bits(r.d[0] as u32),
                    depth_write: r.x[2] as u32,
                    stencil: r.x[3] as u32,
                    stencil_mask: r.x[4] as u32,
                },
            )
        },
        "nvnCommandBufferSetViewport" => |i, _, r| {
            record(
                i,
                0,
                r,
                RecordedCommand::SetViewport(r.x[..5].try_into().unwrap()),
            )
        },
        "nvnCommandBufferSetScissor" => |i, _, r| {
            record(
                i,
                0,
                r,
                RecordedCommand::SetScissor(r.x[..5].try_into().unwrap()),
            )
        },
        "nvnCommandBufferSetDepthRange" => |i, _, r| {
            record(
                i,
                0,
                r,
                RecordedCommand::SetDepthRange([r.x[1], r.d[0], r.d[1]]),
            )
        },
        // Kept for a later executor, but reported as not implemented: the
        // host then treats the call exactly as before this handler existed,
        // which a program was seen to depend on (note 0012).
        _ => |instance, function, r| {
            let _ = record(
                instance,
                function.0,
                r,
                RecordedCommand::Raw {
                    function: function.0,
                    registers: r.x,
                },
            );
            Status::Unimplemented
        },
    })
}
