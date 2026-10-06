//! The optional Vulkan execution context.
//!
//! This module contains only public Vulkan pieces and novena's own bookkeeping.
//! It deliberately does not describe or reproduce the target API.

use ash::{vk, Device, Entry, Instance};
use std::collections::HashMap;
use std::ffi::CString;

/// A device and its one graphics queue. Failure is represented as `None` so a
/// host without Vulkan keeps using the CPU executor.
pub struct Context {
    pub entry: Entry,
    pub instance: Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
}

pub struct Backend {
    // Fields drop in declaration order. The images belong to the device in
    // `context`, so they must go first.
    pub images: Images,
    context: Context,
}

impl Backend {
    pub fn new(scale: f32) -> Option<Self> {
        let context = Context::new()?;
        let images = Images::new(&context, scale)?;
        Some(Self { context, images })
    }
    pub fn ensure(&mut self, key: u64, width: u64, height: u64, depth: bool) -> bool {
        self.images.ensure(&self.context, key, width, height, depth)
    }
    pub fn clear_color(&mut self, key: u64, color: [f32; 4], mask: u32) -> bool {
        self.images.clear_color(&self.context, key, color, mask)
    }
    pub fn clear_depth(&mut self, key: u64, depth: f32, stencil: u32) -> bool {
        self.images.clear_depth(&self.context, key, depth, stencil)
    }
    pub fn readback(&mut self, key: u64) -> Option<(u32, u32, Vec<u8>)> {
        self.images.readback(&self.context, key)
    }

    /// Copy entry points are kept behind the backend boundary. The current
    /// Vulkan image allocator has no host-visible staging allocation yet, so
    /// unresolved copy descriptors are deliberately reported as unsupported.
    pub fn upload(&mut self, _key: u64, _data: &[u8], _width: u64, _height: u64) -> bool {
        false
    }

    pub fn copy(&mut self, _destination: u64, _source: u64) -> bool {
        false
    }
}

impl Context {
    /// Create a context, preferring a discrete GPU and then any usable GPU.
    pub fn new() -> Option<Self> {
        // SAFETY: loading the system Vulkan loader is the boundary of this
        // optional backend; ash validates the returned function table.
        let entry = unsafe { Entry::load().ok()? };
        let app = CString::new("novena").ok()?;
        let info = vk::ApplicationInfo::default()
            .application_name(&app)
            .engine_name(&app)
            .api_version(vk::make_api_version(0, 1, 0, 0));
        let create = vk::InstanceCreateInfo::default().application_info(&info);
        // SAFETY: `create` points only to local, immutable Vulkan structs.
        let instance = unsafe { entry.create_instance(&create, None).ok()? };
        // SAFETY: the instance is live and owns this enumeration.
        let devices = unsafe { instance.enumerate_physical_devices().ok()? };
        let pick = devices
            .iter()
            .copied()
            .find(|&device| {
                // SAFETY: device was returned by this live instance.
                unsafe {
                    instance.get_physical_device_properties(device).device_type
                        == vk::PhysicalDeviceType::DISCRETE_GPU
                }
            })
            .or_else(|| devices.first().copied())?;
        // SAFETY: device was returned by this live instance.
        let families = unsafe { instance.get_physical_device_queue_family_properties(pick) };
        let family = families
            .iter()
            .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))?
            as u32;
        let priority = [1.0_f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priority)];
        let device_info = vk::DeviceCreateInfo::default().queue_create_infos(&queue_info);
        // SAFETY: the selected family supports graphics and the create info is valid.
        let device = unsafe { instance.create_device(pick, &device_info, None).ok()? };
        // SAFETY: queue zero was requested above.
        let queue = unsafe { device.get_device_queue(family, 0) };
        Some(Self {
            entry,
            instance,
            physical_device: pick,
            device,
            queue,
            queue_family: family,
        })
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: all work submitted by this small context is complete before
        // it is dropped by the owning instance.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// Vulkan images owned by textures novena renders to.
pub struct Images {
    images: HashMap<u64, Image>,
    scale: f32,
    command_pool: vk::CommandPool,
    queue: vk::Queue,
    device: Device,
}

struct Image {
    image: vk::Image,
    memory: vk::DeviceMemory,
    width: u32,
    height: u32,
}

impl Images {
    pub fn new(context: &Context, scale: f32) -> Option<Self> {
        let info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(context.queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        // SAFETY: the queue family belongs to the live device.
        let command_pool = unsafe { context.device.create_command_pool(&info, None).ok()? };
        Some(Self {
            images: HashMap::new(),
            scale,
            command_pool,
            queue: context.queue,
            device: context.device.clone(),
        })
    }

    fn dimensions(&self, width: u64, height: u64) -> (u32, u32) {
        let scale = self.scale.max(1.0);
        (
            ((width.max(1) as f32 * scale).round() as u32).max(1),
            ((height.max(1) as f32 * scale).round() as u32).max(1),
        )
    }

    pub fn ensure(
        &mut self,
        context: &Context,
        key: u64,
        width: u64,
        height: u64,
        depth: bool,
    ) -> bool {
        if self.images.contains_key(&key) {
            return true;
        }
        let (width, height) = self.dimensions(width, height);
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            // RGBA8 and D32 are novena's own choices until texture formats are observed.
            .format(if depth {
                vk::Format::D32_SFLOAT
            } else {
                vk::Format::R8G8B8A8_UNORM
            })
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(if depth {
                vk::ImageUsageFlags::TRANSFER_DST
            } else {
                vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST
            })
            .initial_layout(vk::ImageLayout::UNDEFINED);
        // SAFETY: image parameters are valid for the selected device.
        let Some(image) = (unsafe { self.device.create_image(&image_info, None).ok() }) else {
            return false;
        };
        // SAFETY: image is live and owned by this device.
        let requirements = unsafe { self.device.get_image_memory_requirements(image) };
        let Some(memory_type) = find_memory_type(
            context,
            requirements.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        ) else {
            return false;
        };
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type);
        // SAFETY: allocation follows the image's requirements.
        let Some(memory) = (unsafe { self.device.allocate_memory(&alloc, None).ok() }) else {
            return false;
        };
        // SAFETY: image and memory are compatible and offsets are zero.
        if unsafe { self.device.bind_image_memory(image, memory, 0).is_err() } {
            return false;
        }
        self.images.insert(
            key,
            Image {
                image,
                memory,
                width,
                height,
            },
        );
        true
    }

    pub fn clear_color(&mut self, context: &Context, key: u64, color: [f32; 4], mask: u32) -> bool {
        let Some(target) = self.images.get(&key) else {
            return false;
        };
        let clear = vk::ClearColorValue { float32: color };
        self.one_shot(
            context,
            target.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            |device, cmd, image| unsafe {
                let barrier = image_barrier(
                    device,
                    image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    false,
                );
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
                device.cmd_clear_color_image(
                    cmd,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &clear,
                    &[vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }],
                );
            },
        ) && mask != 0
    }

    pub fn clear_depth(&mut self, context: &Context, key: u64, depth: f32, stencil: u32) -> bool {
        let Some(target) = self.images.get(&key) else {
            return false;
        };
        let clear = vk::ClearDepthStencilValue { depth, stencil };
        self.one_shot(
            context,
            target.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            |device, cmd, image| unsafe {
                let barrier = image_barrier(
                    device,
                    image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    true,
                );
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
                device.cmd_clear_depth_stencil_image(
                    cmd,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &clear,
                    &[vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::DEPTH,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    }],
                );
            },
        ) && context.queue != vk::Queue::null()
    }

    pub fn readback(&mut self, context: &Context, key: u64) -> Option<(u32, u32, Vec<u8>)> {
        let target = self.images.get(&key)?;
        let size = u64::from(target.width) * u64::from(target.height) * 4;
        let buffer_info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(vk::BufferUsageFlags::TRANSFER_DST)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: buffer parameters are valid.
        let buffer = unsafe { self.device.create_buffer(&buffer_info, None).ok()? };
        // SAFETY: buffer is live.
        let req = unsafe { self.device.get_buffer_memory_requirements(buffer) };
        let ty = find_memory_type(
            context,
            req.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size)
            .memory_type_index(ty);
        // SAFETY: allocation follows the buffer's requirements.
        let memory = unsafe { self.device.allocate_memory(&alloc, None).ok()? };
        // SAFETY: compatible buffer memory.
        unsafe { self.device.bind_buffer_memory(buffer, memory, 0).ok()? };
        let ok = self.one_shot(
            context,
            target.image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            |device, cmd, image| unsafe {
                let barrier = image_barrier(
                    device,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    false,
                );
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
                device.cmd_copy_image_to_buffer(
                    cmd,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buffer,
                    &[vk::BufferImageCopy {
                        buffer_offset: 0,
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: vk::ImageSubresourceLayers {
                            aspect_mask: vk::ImageAspectFlags::COLOR,
                            mip_level: 0,
                            base_array_layer: 0,
                            layer_count: 1,
                        },
                        image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                        image_extent: vk::Extent3D {
                            width: target.width,
                            height: target.height,
                            depth: 1,
                        },
                    }],
                );
            },
        );
        let result = if ok {
            unsafe {
                let ptr = self
                    .device
                    .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())
                    .ok()?;
                let data = std::slice::from_raw_parts(ptr.cast::<u8>(), size as usize).to_vec();
                self.device.unmap_memory(memory);
                Some((target.width, target.height, data))
            }
        } else {
            None
        };
        // SAFETY: no command uses these objects after one_shot waits.
        unsafe {
            self.device.destroy_buffer(buffer, None);
            self.device.free_memory(memory, None);
        }
        result
    }

    fn one_shot(
        &self,
        _context: &Context,
        image: vk::Image,
        _layout: vk::ImageLayout,
        record: impl FnOnce(&Device, vk::CommandBuffer, vk::Image),
    ) -> bool {
        let alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(self.command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: command pool is live.
        let Ok(commands) = (unsafe { self.device.allocate_command_buffers(&alloc) }) else {
            return false;
        };
        let cmd = commands[0];
        // SAFETY: command buffer is newly allocated.
        if unsafe {
            self.device
                .begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default())
        }
        .is_err()
        {
            return false;
        }
        record(&self.device, cmd, image);
        // SAFETY: recording was begun above.
        if unsafe { self.device.end_command_buffer(cmd) }.is_err() {
            return false;
        }
        let submit = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd));
        // SAFETY: queue and command buffer are live.
        let ok = unsafe {
            self.device
                .queue_submit(self.queue, std::slice::from_ref(&submit), vk::Fence::null())
                .is_ok()
                && self.device.queue_wait_idle(self.queue).is_ok()
        };
        // SAFETY: command has completed.
        unsafe {
            self.device
                .free_command_buffers(self.command_pool, &commands);
        }
        ok
    }
}

impl Default for Images {
    fn default() -> Self {
        panic!("Vulkan Images require a context")
    }
}

fn find_memory_type(context: &Context, bits: u32, flags: vk::MemoryPropertyFlags) -> Option<u32> {
    let memory = unsafe {
        context
            .instance
            .get_physical_device_memory_properties(context.physical_device)
    };
    (0..memory.memory_type_count).find(|&i| {
        bits & (1 << i) != 0
            && memory.memory_types[i as usize]
                .property_flags
                .contains(flags)
    })
}

fn image_barrier(
    _device: &Device,
    image: vk::Image,
    old: vk::ImageLayout,
    new: vk::ImageLayout,
    depth: bool,
) -> vk::ImageMemoryBarrier<'static> {
    vk::ImageMemoryBarrier::default()
        .old_layout(old)
        .new_layout(new)
        .image(image)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: if depth {
                vk::ImageAspectFlags::DEPTH
            } else {
                vk::ImageAspectFlags::COLOR
            },
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        })
}

impl Drop for Images {
    fn drop(&mut self) {
        // SAFETY: all one-shot work has completed before the backend is dropped.
        unsafe {
            for (_, image) in self.images.drain() {
                self.device.destroy_image(image.image, None);
                self.device.free_memory(image.memory, None);
            }
            self.device.destroy_command_pool(self.command_pool, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Backend;

    #[test]
    fn clear_and_readback_at_scale_one() {
        let Some(mut backend) = Backend::new(1.0) else {
            return;
        };
        assert!(backend.ensure(1, 4, 4, false));
        assert!(backend.clear_color(1, [1.0, 0.5, 0.0, 1.0], 0xf));
        let (width, height, pixels) = backend.readback(1).expect("Vulkan readback");
        assert_eq!((width, height), (4, 4));
        assert_eq!(&pixels[..4], &[255, 128, 0, 255]);
    }

    #[test]
    fn clear_and_readback_at_scale_two() {
        let Some(mut backend) = Backend::new(2.0) else {
            return;
        };
        assert!(backend.ensure(1, 2, 1, false));
        assert!(backend.clear_color(1, [0.0, 0.0, 1.0, 1.0], 0xf));
        let (width, height, pixels) = backend.readback(1).expect("Vulkan readback");
        assert_eq!((width, height), (4, 2));
        assert_eq!(&pixels[..4], &[0, 0, 255, 255]);
    }
}
