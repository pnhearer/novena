//! Queue, sync, event and window objects.
//! Signatures: docs/signatures/0001-setup.md, 0003-objects.md, 0004-pointers.md.

use super::{accept, objects::Object, read_u64, succeed, write_u32, Handler};
use crate::instance::{Instance, Registers, Status};

fn builder_update(
    instance: &Instance,
    registers: &Registers,
    change: impl FnOnce(&mut Object),
) -> Status {
    instance.objects.update(registers.x[0], change);
    Status::Ok
}

pub fn handler(name: &str) -> Option<Handler> {
    Some(match name {
        "nvnQueueBuilderSetDefaults" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::QueueBuilder {
                    device: 0,
                    control_memory_size: 0,
                    command_flush_threshold: 0,
                },
            );
            Status::Ok
        },
        "nvnQueueBuilderSetDevice" => |instance, _, registers| {
            let value = registers.x[1];
            builder_update(instance, registers, |object| {
                if let Object::QueueBuilder { device, .. } = object {
                    *device = value;
                }
            })
        },
        "nvnQueueBuilderSetControlMemorySize" => |instance, _, registers| {
            let value = registers.x[1];
            builder_update(instance, registers, |object| {
                if let Object::QueueBuilder {
                    control_memory_size,
                    ..
                } = object
                {
                    *control_memory_size = value;
                }
            })
        },
        "nvnQueueBuilderSetCommandFlushThreshold" => |instance, _, registers| {
            let value = registers.x[1];
            builder_update(instance, registers, |object| {
                if let Object::QueueBuilder {
                    command_flush_threshold,
                    ..
                } = object
                {
                    *command_flush_threshold = value;
                }
            })
        },
        "nvnQueueInitialize" => |instance, _, registers| {
            let device = match instance.objects.get(registers.x[1]) {
                Some(Object::QueueBuilder { device, .. }) => device,
                _ => 0,
            };
            instance
                .objects
                .put(registers.x[0], Object::Queue { device });
            succeed(registers)
        },
        // Nothing is drawn yet, so submitting, flushing and finishing have
        // nothing to do. They are accepted so the program keeps going.
        "nvnQueueSubmitCommands"
        | "nvnQueueFlush"
        | "nvnQueueFinish"
        | "nvnQueuePresentTexture" => accept,
        // Observed to return 1. Waiting on a sync that nothing signals would
        // block the program forever, so the wait is reported as satisfied.
        "nvnQueueWaitSync" => |_instance, _, registers| succeed(registers),
        "nvnSyncInitialize" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::Sync {
                    device: registers.x[1],
                },
            );
            succeed(registers)
        },
        "nvnSyncFinalize" => |instance, _, registers| {
            instance.objects.remove(registers.x[0]);
            Status::Ok
        },
        // Observed to return 0, taken to mean the wait completed.
        "nvnSyncWait" => accept,
        "nvnEventBuilderSetStorage" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::EventBuilder {
                    pool: registers.x[1],
                    offset: registers.x[2],
                },
            );
            Status::Ok
        },
        "nvnEventInitialize" => |instance, _, registers| {
            instance
                .objects
                .put(registers.x[0], Object::Event { value: 0 });
            succeed(registers)
        },
        "nvnEventGetValue" => |instance, _, registers| {
            registers.x[0] = match instance.objects.get(registers.x[0]) {
                Some(Object::Event { value }) => u64::from(value),
                _ => 0,
            };
            Status::Ok
        },
        // Signatures 0003 leaves the arguments open. The event is marked
        // signalled, which is the least the name promises.
        "nvnEventSignal" => |instance, _, registers| {
            instance.objects.update(registers.x[0], |object| {
                if let Object::Event { value } = object {
                    *value = 1;
                }
            });
            Status::Ok
        },
        "nvnWindowBuilderSetDefaults" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::WindowBuilder {
                    device: 0,
                    native_window: 0,
                    textures: Vec::new(),
                },
            );
            Status::Ok
        },
        "nvnWindowBuilderSetDevice" => |instance, _, registers| {
            let value = registers.x[1];
            builder_update(instance, registers, |object| {
                if let Object::WindowBuilder { device, .. } = object {
                    *device = value;
                }
            })
        },
        "nvnWindowBuilderSetNativeWindow" => |instance, _, registers| {
            let value = registers.x[1];
            builder_update(instance, registers, |object| {
                if let Object::WindowBuilder { native_window, .. } = object {
                    *native_window = value;
                }
            })
        },
        "nvnWindowBuilderSetTextures" => |instance, _, registers| {
            // An array of `count` texture object addresses (signatures 0004).
            let count = registers.x[1].min(16);
            let mut list = Vec::with_capacity(count as usize);
            for index in 0..count {
                match read_u64(instance, registers.x[2] + index * 8) {
                    Some(address) => list.push(address),
                    None => return Status::BadArgument,
                }
            }
            builder_update(instance, registers, |object| {
                if let Object::WindowBuilder { textures, .. } = object {
                    *textures = list;
                }
            })
        },
        "nvnWindowInitialize" => |instance, _, registers| {
            let (device, textures) = match instance.objects.get(registers.x[1]) {
                Some(Object::WindowBuilder {
                    device, textures, ..
                }) => (device, textures),
                _ => (0, Vec::new()),
            };
            instance.objects.put(
                registers.x[0],
                Object::Window {
                    device,
                    textures,
                    present_interval: 1,
                    next_texture: 0,
                },
            );
            succeed(registers)
        },
        "nvnWindowFinalize" => |instance, _, registers| {
            instance.objects.remove(registers.x[0]);
            Status::Ok
        },
        "nvnWindowSetPresentInterval" => |instance, _, registers| {
            let value = registers.x[1] as u32;
            builder_update(instance, registers, |object| {
                if let Object::Window {
                    present_interval, ..
                } = object
                {
                    *present_interval = value;
                }
            })
        },
        "nvnWindowGetPresentInterval" => |instance, _, registers| {
            registers.x[0] = match instance.objects.get(registers.x[0]) {
                Some(Object::Window {
                    present_interval, ..
                }) => u64::from(present_interval),
                _ => 1,
            };
            Status::Ok
        },
        // Hands out the window's textures in turn and writes the index
        // through the third argument (signatures 0004); returns 0 as
        // observed.
        "nvnWindowAcquireTexture" => |instance, _, registers| {
            let mut index = 0;
            let found = instance.objects.update(registers.x[0], |object| {
                if let Object::Window {
                    textures,
                    next_texture,
                    ..
                } = object
                {
                    index = *next_texture;
                    let count = textures.len().max(1) as u32;
                    *next_texture = (*next_texture + 1) % count;
                }
            });
            if !found || !write_u32(instance, registers.x[2], index) {
                return Status::BadArgument;
            }
            registers.x[0] = 0;
            Status::Ok
        },
        _ => return None,
    })
}
