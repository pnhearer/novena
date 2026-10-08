//! Render images and transfers to canonical arena bytes. Provenance: 0026.

use super::{commands::Commands, find_memory_type, Context};
use ash::{vk, Device};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Copy)]
pub(super) struct ImageInfo {
    pub image: vk::Image,
    pub extent: vk::Extent3D,
    pub format: vk::Format,
    pub layout: vk::ImageLayout,
}

pub(super) struct Image {
    context: Arc<Context>,
    memory: vk::DeviceMemory,
    pub info: ImageInfo,
}

impl Image {
    pub fn new(
        context: &Arc<Context>,
        width: u32,
        height: u32,
        format: vk::Format,
    ) -> Option<Self> {
        let limits = unsafe {
            context
                .instance
                .get_physical_device_properties(context.physical_device)
        }
        .limits;
        if width == 0
            || height == 0
            || width > limits.max_image_dimension2_d
            || height > limits.max_image_dimension2_d
        {
            return None;
        }
        let extent = vk::Extent3D {
            width,
            height,
            depth: 1,
        };
        let mut usage = vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST;
        if format == vk::Format::R8G8B8A8_UNORM {
            let properties = unsafe {
                context
                    .instance
                    .get_physical_device_format_properties(context.physical_device, format)
            };
            if !properties
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags::COLOR_ATTACHMENT)
            {
                return None;
            }
            usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
        }
        let image = unsafe {
            context
                .device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(extent)
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(usage)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )
                .ok()?
        };
        let mut result = Self {
            context: Arc::clone(context),
            memory: vk::DeviceMemory::null(),
            info: ImageInfo {
                image,
                extent,
                format,
                layout: vk::ImageLayout::UNDEFINED,
            },
        };
        let requirements = unsafe { context.device.get_image_memory_requirements(image) };
        let ty = find_memory_type(
            context,
            requirements.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;
        result.memory = unsafe {
            context
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(requirements.size)
                        .memory_type_index(ty),
                    None,
                )
                .ok()?
        };
        unsafe {
            context
                .device
                .bind_image_memory(image, result.memory, 0)
                .ok()?;
        }
        Some(result)
    }

    pub fn transition(&mut self, command: vk::CommandBuffer, layout: vk::ImageLayout) {
        transition(&self.context.device, command, self.info, layout);
        self.info.layout = layout;
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        unsafe {
            // Includes presentation copies and callers using the context directly.
            let _ = self.context.device.device_wait_idle();
            self.context.device.destroy_image(self.info.image, None);
            self.context.device.free_memory(self.memory, None);
        }
    }
}

pub(super) struct Buffer {
    context: Arc<Context>,
    memory: vk::DeviceMemory,
    pub buffer: vk::Buffer,
    size: u64,
}

impl Buffer {
    pub fn new(context: &Arc<Context>, size: u64) -> Option<Self> {
        let buffer = unsafe {
            context
                .device
                .create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(size)
                        .usage(
                            vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST,
                        )
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )
                .ok()?
        };
        let mut result = Self {
            context: Arc::clone(context),
            memory: vk::DeviceMemory::null(),
            buffer,
            size,
        };
        let req = unsafe { context.device.get_buffer_memory_requirements(buffer) };
        let ty = find_memory_type(
            context,
            req.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;
        result.memory = unsafe {
            context
                .device
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(ty),
                    None,
                )
                .ok()?
        };
        unsafe {
            context
                .device
                .bind_buffer_memory(buffer, result.memory, 0)
                .ok()?;
        }
        Some(result)
    }

    pub fn write(&self, bytes: &[u8]) -> Option<()> {
        if bytes.len() as u64 != self.size {
            return None;
        }
        unsafe {
            let ptr = self
                .context
                .device
                .map_memory(self.memory, 0, self.size, vk::MemoryMapFlags::empty())
                .ok()?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast(), bytes.len());
            self.context.device.unmap_memory(self.memory);
        }
        Some(())
    }

    pub fn read(&self) -> Option<Vec<u8>> {
        unsafe {
            let ptr = self
                .context
                .device
                .map_memory(self.memory, 0, self.size, vk::MemoryMapFlags::empty())
                .ok()?;
            let bytes = std::slice::from_raw_parts(ptr.cast::<u8>(), self.size as usize).to_vec();
            self.context.device.unmap_memory(self.memory);
            Some(bytes)
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.context.device.device_wait_idle();
            self.context.device.destroy_buffer(self.buffer, None);
            self.context.device.free_memory(self.memory, None);
        }
    }
}

pub(super) struct Images {
    context: Arc<Context>,
    commands: Commands,
    images: HashMap<u64, Image>,
}

impl Images {
    pub fn new(context: &Arc<Context>) -> Option<Self> {
        Some(Self {
            context: Arc::clone(context),
            commands: Commands::new(context)?,
            images: HashMap::new(),
        })
    }

    pub fn ensure(&mut self, key: u64, width: u64, height: u64, depth: bool) -> bool {
        let Ok(width) = u32::try_from(width.max(1)) else {
            return false;
        };
        let Ok(height) = u32::try_from(height.max(1)) else {
            return false;
        };
        let format = if depth {
            vk::Format::D32_SFLOAT
        } else {
            vk::Format::R8G8B8A8_UNORM
        };
        if let Some(image) = self.images.get(&key) {
            return image.info.extent.width == width
                && image.info.extent.height == height
                && image.info.format == format;
        }
        let Some(image) = Image::new(&self.context, width, height, format) else {
            return false;
        };
        self.images.insert(key, image);
        true
    }

    pub fn remove(&mut self, key: u64) {
        self.images.remove(&key);
    }

    pub fn clear_color(&mut self, key: u64, color: [f32; 4], mask: u32) -> Option<()> {
        if mask & 15 == 0 {
            return Some(());
        }
        if self.images.get(&key)?.info.format != vk::Format::R8G8B8A8_UNORM {
            return None;
        }
        if mask & 15 != 15 {
            // Vulkan transfer clears cannot mask channels. Preserve the others.
            let (_, _, mut bytes) = self.readback(key)?;
            let values = color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            for pixel in bytes.as_chunks_mut::<4>().0 {
                for c in 0..4 {
                    if mask & (1 << c) != 0 {
                        pixel[c] = values[c];
                    }
                }
            }
            return self.upload(key, &bytes);
        }
        let (cmd, _) = self.commands.begin()?;
        let image = self.images.get_mut(&key)?;
        image.transition(cmd, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
        unsafe {
            self.context.device.cmd_clear_color_image(
                cmd,
                image.info.image,
                image.info.layout,
                &vk::ClearColorValue { float32: color },
                &[range(image.info.format)],
            );
        }
        self.commands.submit(false, None)
    }

    pub fn draw(
        &mut self,
        key: u64,
        pipeline: &super::graphics::GraphicsPipeline,
        memory: &super::GlobalMemory,
        draw: &super::graphics::Draw,
    ) -> Option<()> {
        let image = self.images.get(&key)?;
        let device = &self.context.device;
        // Temporary attachment objects remain live through the submission fence.
        let view = unsafe {
            device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image.info.image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(image.info.format)
                        .subresource_range(range(image.info.format)),
                    None,
                )
                .ok()?
        };
        let framebuffer = unsafe {
            device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(pipeline.render_pass)
                    .attachments(&[view])
                    .width(image.info.extent.width)
                    .height(image.info.extent.height)
                    .layers(1),
                None,
            )
        };
        let Ok(framebuffer) = framebuffer else {
            unsafe {
                device.destroy_image_view(view, None);
            }
            return None;
        };
        let result = (|| {
            let (cmd, _) = self.commands.begin()?;
            let image = self.images.get_mut(&key)?;
            buffer_barrier(device, cmd);
            image.transition(cmd, vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
            unsafe {
                device.cmd_begin_render_pass(
                    cmd,
                    &vk::RenderPassBeginInfo::default()
                        .render_pass(pipeline.render_pass)
                        .framebuffer(framebuffer)
                        .render_area(vk::Rect2D::default().extent(vk::Extent2D {
                            width: image.info.extent.width,
                            height: image.info.extent.height,
                        })),
                    vk::SubpassContents::INLINE,
                );
                pipeline.record(cmd, memory, draw);
                device.cmd_end_render_pass(cmd);
            }
            self.commands.submit(false, None)?;
            self.commands.wait()
        })();
        unsafe {
            if result.is_none() {
                let _ = device.device_wait_idle();
            }
            device.destroy_framebuffer(framebuffer, None);
            device.destroy_image_view(view, None);
        }
        result
    }

    pub fn clear_depth(&mut self, key: u64, depth: f32) -> Option<()> {
        if self.images.get(&key)?.info.format != vk::Format::D32_SFLOAT {
            return None;
        }
        let (cmd, _) = self.commands.begin()?;
        let image = self.images.get_mut(&key)?;
        image.transition(cmd, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
        unsafe {
            self.context.device.cmd_clear_depth_stencil_image(
                cmd,
                image.info.image,
                image.info.layout,
                &vk::ClearDepthStencilValue { depth, stencil: 0 },
                &[range(image.info.format)],
            );
        }
        self.commands.submit(false, None)
    }

    pub fn source(&mut self, key: u64) -> Option<ImageInfo> {
        let (cmd, _) = self.commands.begin()?;
        let image = self.images.get_mut(&key)?;
        image.transition(cmd, vk::ImageLayout::TRANSFER_SRC_OPTIMAL);
        let info = image.info;
        self.commands.submit(false, None)?;
        Some(info)
    }

    pub fn arena_transfer(
        &mut self,
        key: u64,
        buffer: vk::Buffer,
        offset: u64,
        load: bool,
    ) -> Option<()> {
        let (cmd, _) = self.commands.begin()?;
        let image = self.images.get_mut(&key)?;
        buffer_barrier(&self.context.device, cmd);
        let layout = if load {
            vk::ImageLayout::TRANSFER_DST_OPTIMAL
        } else {
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL
        };
        image.transition(cmd, layout);
        let region = buffer_region(image.info, offset);
        unsafe {
            if load {
                self.context.device.cmd_copy_buffer_to_image(
                    cmd,
                    buffer,
                    image.info.image,
                    layout,
                    &[region],
                );
            } else {
                self.context.device.cmd_copy_image_to_buffer(
                    cmd,
                    image.info.image,
                    layout,
                    buffer,
                    &[region],
                );
                buffer_barrier(&self.context.device, cmd);
            }
        }
        self.commands.submit(false, None)
    }

    pub fn upload(&mut self, key: u64, bytes: &[u8]) -> Option<()> {
        let info = self.images.get(&key)?.info;
        let staging = Buffer::new(&self.context, image_size(info))?;
        staging.write(bytes)?;
        self.arena_transfer(key, staging.buffer, 0, true)?;
        self.commands.wait()
    }

    pub fn copy(&mut self, destination: u64, source: u64) -> Option<()> {
        if destination == source {
            return Some(());
        }
        let src = self.source(source)?;
        let dst = self.images.get(&destination)?.info;
        if src.extent != dst.extent || src.format != dst.format {
            return None;
        }
        let (cmd, _) = self.commands.begin()?;
        self.images
            .get_mut(&destination)?
            .transition(cmd, vk::ImageLayout::TRANSFER_DST_OPTIMAL);
        unsafe {
            self.context.device.cmd_copy_image(
                cmd,
                src.image,
                src.layout,
                dst.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::ImageCopy::default()
                    .src_subresource(layers(src.format))
                    .dst_subresource(layers(dst.format))
                    .extent(src.extent)],
            );
        }
        self.commands.submit(false, None)
    }

    pub fn readback(&mut self, key: u64) -> Option<(u32, u32, Vec<u8>)> {
        let info = self.images.get(&key)?.info;
        let staging = Buffer::new(&self.context, image_size(info))?;
        self.arena_transfer(key, staging.buffer, 0, false)?;
        self.commands.wait()?;
        Some((info.extent.width, info.extent.height, staging.read()?))
    }

    pub fn read_image(&mut self, image: &mut Image) -> Option<Vec<u8>> {
        let staging = Buffer::new(&self.context, image_size(image.info))?;
        let (cmd, _) = self.commands.begin()?;
        image.transition(cmd, vk::ImageLayout::TRANSFER_SRC_OPTIMAL);
        unsafe {
            self.context.device.cmd_copy_image_to_buffer(
                cmd,
                image.info.image,
                image.info.layout,
                staging.buffer,
                &[buffer_region(image.info, 0)],
            );
        }
        buffer_barrier(&self.context.device, cmd);
        self.commands.submit(false, None)?;
        self.commands.wait()?;
        staging.read()
    }

    pub fn blit_offscreen(&mut self, source: ImageInfo, target: &mut Image) -> Option<()> {
        if !can_blit(&self.context, source.format, target.info.format) {
            return None;
        }
        let (cmd, _) = self.commands.begin()?;
        record_present(
            &self.context.device,
            cmd,
            source,
            target.info,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        );
        target.info.layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
        self.commands.submit(false, None)
    }
}

pub(super) fn image_size(info: ImageInfo) -> u64 {
    u64::from(info.extent.width) * u64::from(info.extent.height) * 4
}

pub(super) fn range(format: vk::Format) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(layers(format).aspect_mask)
        .level_count(1)
        .layer_count(1)
}

fn layers(format: vk::Format) -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers::default()
        .aspect_mask(if format == vk::Format::D32_SFLOAT {
            vk::ImageAspectFlags::DEPTH
        } else {
            vk::ImageAspectFlags::COLOR
        })
        .layer_count(1)
}

fn buffer_region(info: ImageInfo, offset: u64) -> vk::BufferImageCopy {
    vk::BufferImageCopy::default()
        .buffer_offset(offset)
        .image_subresource(layers(info.format))
        .image_extent(info.extent)
}

pub(super) fn transition(
    device: &Device,
    cmd: vk::CommandBuffer,
    image: ImageInfo,
    layout: vk::ImageLayout,
) {
    let (src_stage, src_access) = if image.layout == vk::ImageLayout::UNDEFINED {
        (
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::AccessFlags::empty(),
        )
    } else {
        (
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        )
    };
    let (dst_stage, dst_access) = if layout == vk::ImageLayout::PRESENT_SRC_KHR {
        (
            vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            vk::AccessFlags::empty(),
        )
    } else if layout == vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL {
        (
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        )
    } else {
        (
            vk::PipelineStageFlags::TRANSFER,
            vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::TRANSFER_WRITE,
        )
    };
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[vk::ImageMemoryBarrier::default()
                .src_access_mask(src_access)
                .dst_access_mask(dst_access)
                .old_layout(image.layout)
                .new_layout(layout)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(image.image)
                .subresource_range(range(image.format))],
        );
    }
}

pub(super) fn buffer_barrier(device: &Device, cmd: vk::CommandBuffer) {
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
            vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE | vk::AccessFlags::HOST_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::MEMORY_READ
                        | vk::AccessFlags::MEMORY_WRITE
                        | vk::AccessFlags::HOST_READ,
                )],
            &[],
            &[],
        );
    }
}

pub(super) fn can_blit(context: &Context, source: vk::Format, destination: vk::Format) -> bool {
    unsafe {
        let src = context
            .instance
            .get_physical_device_format_properties(context.physical_device, source);
        let dst = context
            .instance
            .get_physical_device_format_properties(context.physical_device, destination);
        src.optimal_tiling_features
            .contains(vk::FormatFeatureFlags::BLIT_SRC)
            && dst
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags::BLIT_DST)
    }
}

/// Shared by swapchain and offscreen presentation, including conversion and scaling.
pub(super) fn record_present(
    device: &Device,
    cmd: vk::CommandBuffer,
    source: ImageInfo,
    destination: ImageInfo,
    final_layout: vk::ImageLayout,
) {
    transition(device, cmd, source, vk::ImageLayout::TRANSFER_SRC_OPTIMAL);
    transition(
        device,
        cmd,
        destination,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
    );
    let offsets = |extent: vk::Extent3D| {
        [
            vk::Offset3D::default(),
            vk::Offset3D {
                x: extent.width as i32,
                y: extent.height as i32,
                z: 1,
            },
        ]
    };
    unsafe {
        device.cmd_blit_image(
            cmd,
            source.image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            destination.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[vk::ImageBlit::default()
                .src_subresource(layers(source.format))
                .src_offsets(offsets(source.extent))
                .dst_subresource(layers(destination.format))
                .dst_offsets(offsets(destination.extent))],
            vk::Filter::NEAREST,
        );
    }
    transition(
        device,
        cmd,
        ImageInfo {
            layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            ..destination
        },
        final_layout,
    );
}
