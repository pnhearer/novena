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
    let resolution = instance.objects.resolve_gpu_address(address).ok()?;
    if size as u64 > resolution.remaining {
        return None;
    }
    let mut bytes = vec![0; size];
    for (offset, chunk) in bytes.chunks_mut(COPY_CHUNK).enumerate() {
        let address = resolution
            .program_address
            .checked_add((offset * COPY_CHUNK) as u64)?;
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
            for program in instance.pending_programs() {
                crate::api::resources::retry_pending(instance, program, "submit");
            }
            // Only textures a window presents get a CPU image. A program
            // clears thousands of other render targets, and filling CPU
            // copies of all of them stalled the program it was tried on.
            let presented = instance.objects.window_textures();
            #[cfg(feature = "vulkan")]
            let mut gpu = instance.gpu.lock().unwrap_or_else(|p| p.into_inner());
            #[cfg(feature = "vulkan")]
            if let Some(memory) = gpu
                .as_mut()
                .and_then(|backend| backend.global_memory.as_mut())
            {
                if !memory.upload(|address, bytes| instance.read_memory(address, bytes)) {
                    return Status::BadArgument;
                }
            }
            let submitted = (|| {
                for i in 0..r.x[1].min(1024) {
                    let Some(handle) = read_u64(instance, r.x[2] + i * 8) else {
                        return Status::BadArgument;
                    };
                    let commands = instance.objects.recording(handle);
                    let Some(commands) = commands else {
                        continue;
                    };
                    let mut targets = Vec::new();
                    let mut depth_target = 0;
                    let mut target_views = [0, 0];
                    #[cfg(feature = "vulkan")]
                    let mut draw_state = super::drawing::State::default();
                    for command in commands {
                        match command {
                            RecordedCommand::SetRenderTargets {
                                colors,
                                depth,
                                views,
                            } => {
                                targets = colors;
                                depth_target = depth;
                                target_views = views;
                            }
                            RecordedCommand::ClearColor { index, color, mask } => {
                                let Some(texture) = targets.get(index as usize).copied() else {
                                    continue;
                                };
                                let Some(Object::Texture { description, image }) =
                                    instance.objects.get(texture)
                                else {
                                    return Status::BadArgument;
                                };
                                #[cfg(feature = "vulkan")]
                                if let Some(backend) = gpu.as_mut() {
                                    if !backend.ensure_texture(texture, &description, false) {
                                        return Status::BadArgument;
                                    }
                                    if !backend.clear_color(texture, color, mask) {
                                        return Status::InternalError;
                                    }
                                    continue;
                                }
                                if !presented.contains(&texture) {
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
                                let values =
                                    color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                                for pixel in image.pixels.as_chunks_mut::<4>().0 {
                                    for c in 0..4 {
                                        if mask & (1 << c) != 0 {
                                            pixel[c] = values[c];
                                        }
                                    }
                                }
                            }
                            RecordedCommand::ClearDepthStencil {
                                depth,
                                depth_write,
                                stencil,
                                stencil_mask,
                            } => {
                                #[cfg(not(feature = "vulkan"))]
                                let _ = (depth, depth_write, stencil, stencil_mask, depth_target);
                                let _ = stencil;
                                #[cfg(feature = "vulkan")]
                                if let Some(backend) = gpu.as_mut() {
                                    if stencil_mask != 0 {
                                        return Status::Unimplemented;
                                    }
                                    if depth_write == 0 || depth_target == 0 {
                                        continue;
                                    }
                                    let Some(Object::Texture { description, .. }) =
                                        instance.objects.get(depth_target)
                                    else {
                                        return Status::BadArgument;
                                    };
                                    if !backend.ensure_texture(depth_target, &description, true) {
                                        return Status::BadArgument;
                                    }
                                    if !backend.clear_depth(depth_target, depth, 0) {
                                        return Status::InternalError;
                                    }
                                }
                            }
                            RecordedCommand::CopyBufferToTexture { buffer, texture } => {
                                let Some(Object::Texture { description, .. }) =
                                    instance.objects.get(texture)
                                else {
                                    return Status::BadArgument;
                                };
                                #[cfg(feature = "vulkan")]
                                if let Some(backend) = gpu.as_mut() {
                                    let size = if backend.has_image_contract() {
                                        let Some(r) = backend.resolved_image(&description) else {
                                            return Status::BadArgument;
                                        };
                                        r.packing.linear_size()
                                    } else {
                                        let Some(size) = copy_size(&description) else {
                                            return Status::BadArgument;
                                        };
                                        size
                                    };
                                    if !backend.ensure_texture(texture, &description, false) {
                                        return Status::BadArgument;
                                    }
                                    let Ok(source) = instance.objects.resolve_gpu_address(buffer)
                                    else {
                                        return Status::BadArgument;
                                    };
                                    if size as u64 > source.remaining {
                                        return Status::BadArgument;
                                    }
                                    if !backend.copy_from_arena(
                                        texture,
                                        source.pool,
                                        source.offset,
                                        size,
                                    ) {
                                        return Status::InternalError;
                                    }
                                    continue;
                                }
                                let Some(size) = copy_size(&description) else {
                                    return Status::BadArgument;
                                };
                                let Some(data) = read_pool_bytes(instance, buffer, size) else {
                                    return Status::BadArgument;
                                };
                                copy_cpu(instance, buffer, texture, data);
                            }
                            RecordedCommand::CopyTextureToTexture {
                                source,
                                destination,
                                arguments,
                            } => {
                                #[cfg(not(feature = "vulkan"))]
                                let _ = arguments;
                                #[cfg(feature = "vulkan")]
                                if let Some(backend) = gpu.as_mut() {
                                    if let Some(decoder) = &backend.copy_decoder {
                                        let Some(operation) = decoder(arguments) else {
                                            return Status::Unimplemented;
                                        };
                                        use crate::gpu::image_layout::CopyOperation;
                                        let (src, dst) = match operation {
                                            CopyOperation::Copy {
                                                source,
                                                destination,
                                                ..
                                            }
                                            | CopyOperation::Blit {
                                                source,
                                                destination,
                                                ..
                                            } => (source, destination),
                                        };
                                        for texture in [src, dst] {
                                            let Some(Object::Texture { description, .. }) =
                                                instance.objects.get(texture)
                                            else {
                                                return Status::BadArgument;
                                            };
                                            if !backend.ensure_texture(texture, &description, false)
                                            {
                                                return Status::BadArgument;
                                            }
                                        }
                                        let ok = match operation {
                                            CopyOperation::Copy { from, to, .. } => {
                                                backend.copy_region(dst, src, from, to)
                                            }
                                            CopyOperation::Blit {
                                                from, to, filter, ..
                                            } => backend.blit_region(dst, src, from, to, filter),
                                        };
                                        if !ok {
                                            return Status::Unimplemented;
                                        }
                                        continue;
                                    }
                                    for texture in [source, destination] {
                                        let Some(Object::Texture { description, .. }) =
                                            instance.objects.get(texture)
                                        else {
                                            return Status::BadArgument;
                                        };
                                        if !backend.ensure_texture(texture, &description, false) {
                                            return Status::BadArgument;
                                        }
                                    }
                                    if !backend.copy(destination, source) {
                                        return Status::InternalError;
                                    }
                                    continue;
                                }
                                let Some(Object::Texture { image, .. }) =
                                    instance.objects.get(source)
                                else {
                                    return Status::BadArgument;
                                };
                                let Some(data) = image
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .as_ref()
                                    .map(|i| i.pixels.clone())
                                else {
                                    return Status::BadArgument;
                                };
                                copy_cpu(instance, source, destination, data);
                            }
                            RecordedCommand::DrawArrays {
                                primitive,
                                first,
                                count,
                            } => {
                                #[cfg(feature = "vulkan")]
                                if let Some(backend) = gpu.as_mut() {
                                    if let Err(status) = draw_state.execute(
                                        instance,
                                        backend,
                                        super::drawing::Request {
                                            targets: &targets,
                                            depth: depth_target,
                                            views: target_views,
                                            primitive,
                                            vertices: super::drawing::Vertices::Arrays { first },
                                            count,
                                        },
                                    ) {
                                        return status;
                                    }
                                    continue;
                                }
                                let _ = (primitive, first, count, target_views);
                                return Status::Unimplemented;
                            }
                            RecordedCommand::DrawElementsBaseVertex {
                                primitive,
                                index_type,
                                count,
                                indices,
                                base_vertex,
                            } => {
                                #[cfg(feature = "vulkan")]
                                if let Some(backend) = gpu.as_mut() {
                                    if let Err(status) = draw_state.execute(
                                        instance,
                                        backend,
                                        super::drawing::Request {
                                            targets: &targets,
                                            depth: depth_target,
                                            views: target_views,
                                            primitive,
                                            count,
                                            vertices: super::drawing::Vertices::Elements {
                                                index_type,
                                                indices,
                                                base_vertex,
                                            },
                                        },
                                    ) {
                                        return status;
                                    }
                                    continue;
                                }
                                let _ = (
                                    primitive,
                                    index_type,
                                    count,
                                    indices,
                                    base_vertex,
                                    target_views,
                                );
                                return Status::Unimplemented;
                            }
                            RecordedCommand::DrawArraysInstanced { .. } => {
                                return Status::Unimplemented
                            }
                            command @ (RecordedCommand::SetViewport(_)
                            | RecordedCommand::SetScissor(_)
                            | RecordedCommand::SetDepthRange(_)
                            | RecordedCommand::BindProgram(_)
                            | RecordedCommand::BindState { .. }
                            | RecordedCommand::Raw { .. }
                            | RecordedCommand::State(_)) => {
                                #[cfg(feature = "vulkan")]
                                draw_state.record(command);
                                #[cfg(not(feature = "vulkan"))]
                                let _ = command;
                            }
                        }
                    }
                }
                Status::Ok
            })();
            #[cfg(feature = "vulkan")]
            if let Some(memory) = gpu
                .as_mut()
                .and_then(|backend| backend.global_memory.as_mut())
            {
                if !memory.download(|address, bytes| instance.write_memory(address, bytes)) {
                    return Status::BadArgument;
                }
            }
            if submitted != Status::Ok {
                return submitted;
            }
            r.x[0] = 0;
            Status::Ok
        },
        "nvnQueueFlush" => |instance, _, registers| {
            for program in instance.pending_programs() {
                crate::api::resources::retry_pending(instance, program, "flush");
            }
            accept(
                instance,
                crate::functions::lookup("nvnQueueFlush").unwrap(),
                registers,
            )
        },
        "nvnQueueFinish" => |instance, _, registers| {
            #[cfg(feature = "vulkan")]
            if instance
                .gpu
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .is_some_and(|backend| !backend.finish())
            {
                return Status::InternalError;
            }
            for program in instance.pending_programs() {
                crate::api::resources::retry_pending(instance, program, "finish");
            }
            accept(
                instance,
                crate::functions::lookup("nvnQueueFinish").unwrap(),
                registers,
            )
        },
        "nvnQueuePresentTexture" => |instance, _, r| {
            let window = r.x[1];
            let Some(Object::Window {
                textures,
                present_interval,
                ..
            }) = instance.objects.get(window)
            else {
                return Status::BadArgument;
            };
            let Some(texture) = textures.get(r.x[2] as usize).copied() else {
                return Status::BadArgument;
            };
            let Some(Object::Texture { description, image }) = instance.objects.get(texture) else {
                return Status::BadArgument;
            };
            #[cfg(not(feature = "vulkan"))]
            let _ = &description;
            let Some(host) = instance.host() else {
                r.x[0] = 0;
                return Status::Ok;
            };
            if let Some(wait) = host.wait_vblank {
                unsafe {
                    wait(host.user);
                }
            }
            #[cfg(feature = "vulkan")]
            if let Some(backend) = instance
                .gpu
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
            {
                if !backend.ensure_texture(texture, &description, false) {
                    return Status::BadArgument;
                }
                if !host.vulkan.is_null() {
                    if backend
                        .present_window(host, window, texture, present_interval)
                        .is_none()
                    {
                        return Status::InternalError;
                    }
                    r.x[0] = 0;
                    return Status::Ok;
                }
                if let Some(present) = host.present {
                    let Some((width, height, pixels)) =
                        backend.present_callback(texture, description.width, description.height)
                    else {
                        return Status::InternalError;
                    };
                    unsafe {
                        present(
                            host.user,
                            window,
                            width,
                            height,
                            pixels.as_ptr(),
                            u64::from(width) * 4,
                        );
                    }
                }
                r.x[0] = 0;
                return Status::Ok;
            }
            let _ = present_interval;
            if !host.vulkan.is_null() {
                return Status::InternalError;
            }
            if let Some(present) = host.present {
                let image = image
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .as_ref()
                    .cloned();
                let Some(image) = image else {
                    return Status::BadArgument;
                };
                unsafe {
                    present(
                        host.user,
                        window,
                        image.width,
                        image.height,
                        image.pixels.as_ptr(),
                        u64::from(image.width) * 4,
                    );
                }
            }
            r.x[0] = 0;
            Status::Ok
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
            let Some(Object::WindowBuilder {
                device,
                textures,
                native_window,
            }) = instance.objects.get(registers.x[1])
            else {
                return Status::BadArgument;
            };
            #[cfg(feature = "vulkan")]
            if let Some(host) = instance.host() {
                if !host.vulkan.is_null() {
                    let mut gpu = instance.gpu.lock().unwrap_or_else(|p| p.into_inner());
                    let Some(backend) = gpu.as_mut() else {
                        registers.x[0] = 0;
                        return Status::InternalError;
                    };
                    if backend
                        .open_window(host, registers.x[0], native_window)
                        .is_none()
                    {
                        registers.x[0] = 0;
                        return Status::InternalError;
                    }
                }
            }
            #[cfg(not(feature = "vulkan"))]
            if instance.host().is_some_and(|host| !host.vulkan.is_null()) {
                registers.x[0] = 0;
                return Status::Unimplemented;
            }
            let _ = native_window;
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
            #[cfg(feature = "vulkan")]
            if let Some(backend) = instance
                .gpu
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
            {
                backend.close_window(registers.x[0]);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Host;
    use std::ffi::c_void;

    unsafe extern "C" fn read(_user: *mut c_void, address: u64, out: *mut u8, size: u64) -> i32 {
        let Some(offset) = address.checked_sub(0x1000) else {
            return 1;
        };
        if offset.checked_add(size).is_none_or(|end| end > 4) {
            return 1;
        }
        let bytes = [10_u8, 20, 30, 40];
        // SAFETY: the callback contract supplies a valid output slice and the
        // checked range lies in this project's fixture.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr().add(offset as usize), out, size as usize)
        };
        0
    }

    #[test]
    fn buffer_copy_resolves_gpu_addresses_before_reading_cpu_storage() {
        // SAFETY: the stateless callback accepts only its synthetic memory range.
        let instance = unsafe {
            Instance::with_host(Host {
                user: std::ptr::null_mut(),
                read_memory: Some(read),
                write_memory: None,
                present: None,
                render_scale: 1.0,
                wait_vblank: None,
                vulkan: std::ptr::null(),
            })
        };
        instance.objects.put(
            1,
            Object::MemoryPool {
                device: 0,
                flags: 0,
                storage: 0x1000,
                size: 4,
                gpu_address: Some(0x10000),
                observed_gpu_address: None,
            },
        );
        assert_eq!(read_pool_bytes(&instance, 0x10001, 2), Some(vec![20, 30]));
        assert_eq!(read_pool_bytes(&instance, 0x10003, 2), None);
        assert_eq!(read_pool_bytes(&instance, 0x1001, 2), None);
    }
}
