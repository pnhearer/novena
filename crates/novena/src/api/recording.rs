//! Command buffers: set-up and recording bookkeeping.
//! Signatures: docs/signatures/0002-command-buffer.md.
//!
//! The commands themselves (binding state, drawing, clearing) have no
//! handlers yet: there is nothing to draw into. Only the calls that manage
//! the command buffer object are handled, so the program can get through
//! its set-up.

use super::{accept, objects::Object, succeed, Handler};
use crate::instance::Status;

pub fn handler(name: &str) -> Option<Handler> {
    Some(match name {
        "nvnCommandBufferInitialize" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::CommandBuffer {
                    device: registers.x[1],
                    command_memory: Vec::new(),
                    control_memory: Vec::new(),
                    recording: false,
                    recordings: 0,
                },
            );
            succeed(registers)
        },
        "nvnCommandBufferFinalize" => |instance, _, registers| {
            instance.objects.remove(registers.x[0]);
            Status::Ok
        },
        "nvnCommandBufferAddCommandMemory" => |instance, _, registers| {
            let entry = (registers.x[1], registers.x[2], registers.x[3]);
            instance.objects.update(registers.x[0], |object| {
                if let Object::CommandBuffer { command_memory, .. } = object {
                    command_memory.push(entry);
                }
            });
            Status::Ok
        },
        "nvnCommandBufferAddControlMemory" => |instance, _, registers| {
            let entry = (registers.x[1], registers.x[2]);
            instance.objects.update(registers.x[0], |object| {
                if let Object::CommandBuffer { control_memory, .. } = object {
                    control_memory.push(entry);
                }
            });
            Status::Ok
        },
        // Observed to answer the size the program had added.
        "nvnCommandBufferGetCommandMemorySize" => |instance, _, registers| {
            registers.x[0] = match instance.objects.get(registers.x[0]) {
                Some(Object::CommandBuffer { command_memory, .. }) => {
                    command_memory.iter().map(|(_, _, size)| size).sum()
                }
                _ => 0,
            };
            Status::Ok
        },
        // The callbacks would ask the program for more memory; novena
        // records no commands and never needs any. Accepted and ignored.
        "nvnCommandBufferSetMemoryCallback"
        | "nvnCommandBufferSetMemoryCallbackData"
        | "nvnCommandBufferSetTexturePool"
        | "nvnCommandBufferSetSamplerPool"
        | "nvnCommandBufferSetShaderScratchMemory" => accept,
        "nvnCommandBufferBeginRecording" => |instance, _, registers| {
            instance.objects.update(registers.x[0], |object| {
                if let Object::CommandBuffer { recording, .. } = object {
                    *recording = true;
                }
            });
            Status::Ok
        },
        // Returns a handle the program later submits. The command buffer's
        // own address, with a count in the top bits so successive recordings
        // differ, is novena's choice; signatures 0002 leaves the real value
        // open.
        "nvnCommandBufferEndRecording" => |instance, _, registers| {
            let mut handle = 0;
            instance.objects.update(registers.x[0], |object| {
                if let Object::CommandBuffer {
                    recording,
                    recordings,
                    ..
                } = object
                {
                    *recording = false;
                    *recordings += 1;
                    handle = registers.x[0] | (*recordings & 0xffff) << 48;
                }
            });
            registers.x[0] = handle;
            Status::Ok
        },
        _ => return None,
    })
}
