//! Queue, sync, event and window objects.
//! Signatures: docs/signatures/0001-setup.md, 0003-objects.md, 0004-pointers.md.

use super::{
    accept,
    objects::{Object, RecordedCommand, TextureDescription, TextureImage},
    read_u64, succeed, write_u32, Handler,
};
use crate::instance::{Instance, Registers, Status};

fn builder_update(
    instance: &Instance,
    registers: &Registers,
    change: impl FnOnce(&mut Object),
) -> Status {
    instance.objects.update(registers.x[0], change);
    Status::Ok
}

const COPY_CHUNK: usize = 64 * 1024;

fn read_pool_bytes(instance: &Instance, address: u64, size: usize) -> Option<Vec<u8>> {
    let mut bytes = vec![0; size];
    for (offset, chunk) in bytes.chunks_mut(COPY_CHUNK).enumerate() {
        let address = address.checked_add((offset * COPY_CHUNK) as u64)?;
        if !instance.read_memory(address, chunk) {
            return None;
        }
    }
    Some(bytes)
}

/// Bytes in a texture's base level at four bytes per texel, or `None` when
/// the recorded size is larger than novena's image limit.
fn copy_size(description: &TextureDescription) -> Option<usize> {
    let texels = description
        .width
        .max(1)
        .checked_mul(description.height.max(1))?
        .checked_mul(description.depth.max(1))?;
    (texels <= CPU_IMAGE_TEXELS).then(|| (texels * 4) as usize)
}

#[cfg(not(feature = "vulkan"))]
fn copy_cpu(instance: &Instance, source: u64, destination: u64, data: Vec<u8>) {
    let Some(Object::Texture { image, description }) = instance.objects.get(destination) else {
        return;
    };
    let level = TextureDescription {
        depth: 1,
        ..description.clone()
    };
    let Some(size) = copy_size(&level) else {
        return;
    };
    let mut image = image.lock().unwrap_or_else(|p| p.into_inner());
    let target = image.get_or_insert_with(|| TextureImage {
        width: description.width.max(1) as u32,
        height: description.height.max(1) as u32,
        depth: 1,
        pixels: vec![0; size],
    });
    let _ = source;
    let length = target.pixels.len().min(data.len());
    target.pixels[..length].copy_from_slice(&data[..length]);
}

/// Marks an event signalled, in novena's record and in the event's storage
/// word in program memory.
/// Largest CPU image, in texels: 4096 by 4096.
const CPU_IMAGE_TEXELS: u64 = 4096 * 4096;

pub(crate) fn signal_event(instance: &Instance, event: u64, new_value: u32) {
    let mut storage = 0;
    instance.objects.update(event, |object| {
        if let Object::Event { value, storage: at } = object {
            *value = new_value;
            storage = *at;
        }
    });
    if storage != 0 {
        write_u32(instance, storage, new_value);
    }
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
        "nvnQueueSubmitCommands" => |instance, _, r| {
            // Only textures a window presents get a CPU image. A program
            // clears thousands of other render targets, and filling CPU
            // copies of all of them stalled the program it was tried on.
            let presented = instance.objects.window_textures();
            #[cfg(feature = "vulkan")]
            let mut gpu = instance.gpu.lock().unwrap_or_else(|p| p.into_inner());
            for i in 0..r.x[1].min(1024) {
                let Some(handle) = read_u64(instance, r.x[2] + i * 8) else {
                    break;
                };
                let commands = instance.objects.recording(handle);
                let Some(commands) = commands else {
                    continue;
                };
                let mut targets = Vec::new();
                for command in commands {
                    match command {
                        RecordedCommand::SetRenderTargets { colors, .. } => targets = colors,
                        RecordedCommand::ClearColor { index, color, mask } => {
                            let Some(texture) = targets.get(index as usize).copied() else {
                                continue;
                            };
                            if !presented.contains(&texture) {
                                #[cfg(feature = "vulkan")]
                                if let Some(backend) = gpu.as_mut() {
                                    let _ = backend.clear_color(texture, color, mask);
                                }
                                continue;
                            }
                            let Some(Object::Texture { description, image }) =
                                instance.objects.get(texture)
                            else {
                                continue;
                            };
                            #[cfg(feature = "vulkan")]
                            if let Some(backend) = gpu.as_mut() {
                                let _ = backend.ensure(
                                    texture,
                                    description.width,
                                    description.height,
                                    false,
                                );
                                let _ = backend.clear_color(texture, color, mask);
                                continue;
                            }
                            // Only the first layer of the base level is kept on the
                            // CPU, and only for sizes a screen can have: a game
                            // clears large arrays and volumes every frame, and
                            // copying those on the CPU stalled it. novena's own
                            // limit, until clears run on the GPU.
                            let (width, height) =
                                (description.width.max(1), description.height.max(1));
                            if width * height > CPU_IMAGE_TEXELS {
                                continue;
                            }
                            let mut image = image.lock().unwrap_or_else(|p| p.into_inner());
                            let image = image.get_or_insert_with(|| TextureImage {
                                width: width as u32,
                                height: height as u32,
                                depth: 1,
                                pixels: vec![0; (width * height * 4) as usize],
                            });
                            // Bits 0..3 mean red, green, blue and alpha. This is novena's own choice.
                            let values = color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                            for pixel in image.pixels.as_chunks_mut::<4>().0 {
                                for c in 0..4 {
                                    if mask & (1 << c) != 0 {
                                        pixel[c] = values[c];
                                    }
                                }
                            }
                        }
                        RecordedCommand::ClearDepthStencil { depth, stencil, .. } => {
                            #[cfg(not(feature = "vulkan"))]
                            let _ = (depth, stencil);
                            let Some(texture) = targets.last().copied() else {
                                continue;
                            };
                            if presented.contains(&texture) {
                                #[cfg(feature = "vulkan")]
                                if let Some(backend) = gpu.as_mut() {
                                    let Some(Object::Texture { description, .. }) =
                                        instance.objects.get(texture)
                                    else {
                                        continue;
                                    };
                                    let _ = backend.ensure(
                                        texture,
                                        description.width,
                                        description.height,
                                        true,
                                    );
                                    let _ = backend.clear_depth(texture, depth, stencil);
                                }
                            }
                        }
                        RecordedCommand::CopyBufferToTexture { buffer, texture } => {
                            let Some(Object::Texture { description, .. }) =
                                instance.objects.get(texture)
                            else {
                                continue;
                            };
                            let Some(size) = copy_size(&description) else {
                                continue;
                            };
                            let Some(data) = read_pool_bytes(instance, buffer, size) else {
                                continue;
                            };
                            #[cfg(feature = "vulkan")]
                            if let Some(backend) = gpu.as_mut() {
                                let _ = backend.upload(
                                    texture,
                                    &data,
                                    description.width,
                                    description.height,
                                );
                            }
                            #[cfg(not(feature = "vulkan"))]
                            copy_cpu(instance, buffer, texture, data);
                        }
                        RecordedCommand::CopyTextureToTexture {
                            source,
                            destination,
                        } => {
                            let Some(Object::Texture { image, .. }) = instance.objects.get(source)
                            else {
                                continue;
                            };
                            let Some(_data) = image
                                .lock()
                                .unwrap_or_else(|p| p.into_inner())
                                .as_ref()
                                .map(|i| i.pixels.clone())
                            else {
                                continue;
                            };
                            #[cfg(feature = "vulkan")]
                            if let Some(backend) = gpu.as_mut() {
                                let _ = backend.copy(destination, source);
                            }
                            #[cfg(not(feature = "vulkan"))]
                            copy_cpu(instance, source, destination, _data);
                        }
                        RecordedCommand::SetViewport(_)
                        | RecordedCommand::SetScissor(_)
                        | RecordedCommand::SetDepthRange(_)
                        | RecordedCommand::DrawArrays { .. }
                        | RecordedCommand::DrawArraysInstanced { .. }
                        | RecordedCommand::DrawElementsBaseVertex { .. }
                        | RecordedCommand::Raw { .. } => {}
                    }
                }
            }
            {
                r.x[0] = 0;
                Status::Ok
            }
        },
        "nvnQueueFlush" | "nvnQueueFinish" => accept,
        "nvnQueuePresentTexture" => |instance, _, r| {
            let Some(Object::Window { textures, .. }) = instance.objects.get(r.x[1]) else {
                return {
                    r.x[0] = 0;
                    Status::Ok
                };
            };
            let Some(texture) = textures.get(r.x[2] as usize).copied() else {
                return {
                    r.x[0] = 0;
                    Status::Ok
                };
            };
            let Some(Object::Texture { description, image }) = instance.objects.get(texture) else {
                return {
                    r.x[0] = 0;
                    Status::Ok
                };
            };
            #[cfg(feature = "vulkan")]
            if let Some(backend) = instance
                .gpu
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
            {
                if let Some((width, height, pixels)) = backend.readback(texture) {
                    if let Some(wait) = instance.host().and_then(|h| h.wait_vblank) {
                        unsafe { wait(instance.host().unwrap().user) };
                    }
                    if let Some(host) = instance.host().and_then(|h| h.present) {
                        unsafe {
                            host(
                                instance.host().unwrap().user,
                                r.x[1],
                                width,
                                height,
                                pixels.as_ptr(),
                                u64::from(width) * 4,
                            )
                        };
                    }
                    r.x[0] = 0;
                    return Status::Ok;
                }
            }
            let Some(image) = image
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .cloned()
            else {
                return {
                    r.x[0] = 0;
                    Status::Ok
                };
            };
            if let Some(host) = instance.host().and_then(|h| h.present) {
                if let Some(wait) = instance.host().and_then(|h| h.wait_vblank) {
                    unsafe { wait(instance.host().unwrap().user) };
                }
                unsafe {
                    host(
                        instance.host().unwrap().user,
                        r.x[1],
                        description.width as u32,
                        description.height as u32,
                        image.pixels.as_ptr(),
                        description.width * 4,
                    );
                }
            }
            {
                r.x[0] = 0;
                Status::Ok
            }
        },
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
        // The event lives in pool memory the program chose
        // (EventBuilderSetStorage), and the program was seen reading that
        // memory directly rather than calling EventGetValue (note 0011).
        "nvnEventInitialize" => |instance, _, registers| {
            let storage = match instance.objects.get(registers.x[1]) {
                Some(Object::EventBuilder { pool, offset }) => match instance.objects.get(pool) {
                    Some(Object::MemoryPool { storage, .. }) => storage + offset,
                    _ => 0,
                },
                _ => 0,
            };
            instance
                .objects
                .put(registers.x[0], Object::Event { value: 0, storage });
            succeed(registers)
        },
        "nvnEventGetValue" => |instance, _, registers| {
            registers.x[0] = match instance.objects.get(registers.x[0]) {
                Some(Object::Event { value, .. }) => u64::from(value),
                _ => 0,
            };
            Status::Ok
        },
        // Signatures 0003 leaves the arguments open. The event is marked
        // signalled, which is the least the name promises.
        "nvnEventSignal" => |instance, _, registers| {
            signal_event(instance, registers.x[0], 1);
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
