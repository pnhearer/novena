//! The optional Vulkan execution context.
//!
//! This module contains only public Vulkan pieces and novena's own bookkeeping.
//! It deliberately does not describe or reproduce the target API.

use ash::{vk, Device, Entry, Instance};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;

mod commands;
pub mod graphics;
pub mod image_enums;
pub mod image_layout;
mod images;
mod memory;
mod pipeline_disk;
mod pipeline_workers;
mod present;
pub mod texture_transfer;
pub mod textures;
pub mod uniforms;
use images::{Image, Images};
use present::Window;
pub mod pipelines;
pub use memory::GlobalMemory;

/// Optional device capabilities available to translated global-memory modules.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShaderFeatures {
    /// Whether storage-buffer 8-bit access is enabled.
    pub storage_buffer8_bit_access: bool,
    /// Whether storage-buffer 16-bit access is enabled.
    pub storage_buffer16_bit_access: bool,
    /// Whether 8-bit shader integer arithmetic is enabled.
    pub shader_int8: bool,
    /// Whether 16-bit shader integer arithmetic is enabled.
    pub shader_int16: bool,
}

impl ShaderFeatures {
    /// Check the optional capabilities used by flat global memory modules.
    /// This is a feature check, not a replacement for SPIR-V validation.
    pub fn validate_global_shader(self, words: &[u32]) -> Result<(), &'static str> {
        if words.len() < 5 || words[0] != 0x0723_0203 {
            return Err("invalid SPIR-V header");
        }
        let mut offset = 5;
        while offset < words.len() {
            let count = (words[offset] >> 16) as usize;
            let opcode = words[offset] & 0xffff;
            if count == 0 || count > words.len() - offset {
                return Err("invalid SPIR-V instruction length");
            }
            if opcode == 17 {
                if count != 2 {
                    return Err("invalid OpCapability length");
                }
                match words[offset + 1] {
                    39 if !self.shader_int8 => return Err("shaderInt8 is unavailable"),
                    22 if !self.shader_int16 => return Err("shaderInt16 is unavailable"),
                    4448 if !self.storage_buffer8_bit_access => {
                        return Err("storageBuffer8BitAccess is unavailable")
                    }
                    4433 if !self.storage_buffer16_bit_access => {
                        return Err("storageBuffer16BitAccess is unavailable")
                    }
                    _ => {}
                }
            }
            offset += count;
        }
        Ok(())
    }
}

/// A device and its one graphics queue. Failure is represented as `None` so a
/// host without Vulkan keeps using the CPU executor.
pub struct Context {
    /// Loaded Vulkan entry points.
    pub entry: Entry,
    /// Owned Vulkan instance; the context destroys it on drop.
    pub instance: Instance,
    /// Physical device selected for execution.
    pub physical_device: vk::PhysicalDevice,
    /// Owned logical device; the context destroys it on drop.
    pub device: Device,
    /// Graphics queue selected for submission.
    pub queue: vk::Queue,
    /// Queue family index of the graphics queue.
    pub queue_family: u32,
    /// Optional narrow storage and integer capabilities enabled on the device.
    pub shader_features: ShaderFeatures,
    /// Indices of families for which the device created queues.
    pub queue_families: Vec<u32>,
    swapchain: bool,
}

/// Vulkan images, presentation, pipelines, and lazily allocated arena storage.
pub struct Backend {
    pub(crate) first_draw: Option<graphics::FirstDrawContract>,
    pub(crate) texture_contract: textures::TextureContract,
    pub(crate) image_contract: image_layout::ImageContract,
    pub(crate) copy_decoder: Option<image_layout::CopyDecoder>,
    pub(crate) blend_contract: Option<graphics::BlendContract>,
    samplers: HashMap<(u64, u32), textures::Sampler>,
    pub(crate) uniforms: uniforms::UniformBufferContract,
    pub(crate) graphics: graphics::GraphicsPipelines,
    windows: HashMap<u64, Window>,
    offscreen: Option<Image>,
    images: Images,
    bindings: HashMap<u64, (u64, u64, usize)>,
    tiled: HashMap<u64, crate::tiling::Layout>,
    /// Flat mapped arena, present after the first successful pool allocation.
    pub global_memory: Option<GlobalMemory>,
    context: Arc<Context>,
    scale: f32,
}

impl Backend {
    /// Create a backend with output scale at least one; return None on setup failure.
    pub fn new(scale: f32) -> Option<Self> {
        Self::from_context(Arc::new(Context::new()?), scale)
    }

    fn from_context(context: Arc<Context>, scale: f32) -> Option<Self> {
        let images = Images::new(&context)?;
        Some(Self {
            first_draw: None,
            texture_contract: textures::TextureContract::default(),
            image_contract: image_layout::ImageContract::default(),
            copy_decoder: None,
            blend_contract: None,
            samplers: HashMap::new(),
            uniforms: uniforms::UniformBufferContract::default(),
            graphics: graphics::GraphicsPipelines::new(&context)?,
            windows: HashMap::new(),
            offscreen: None,
            images,
            bindings: HashMap::new(),
            tiled: HashMap::new(),
            context,
            scale: scale.max(1.0),
            global_memory: None,
        })
    }

    /// The caller has accepted Host's pointer and callback lifetime contract.
    pub(crate) fn with_host(host: &crate::Host) -> Option<Self> {
        if host.vulkan.is_null() {
            return Self::new(host.render_scale);
        }
        let surface_host = unsafe { &*host.vulkan };
        if surface_host.extension_count == 0
            || surface_host.extension_count > 64
            || surface_host.extensions.is_null()
        {
            return None;
        }
        let pointers = unsafe {
            std::slice::from_raw_parts(
                surface_host.extensions,
                surface_host.extension_count as usize,
            )
        };
        let mut extensions = Vec::new();
        for &pointer in pointers {
            if pointer.is_null() {
                return None;
            }
            extensions.push(unsafe { std::ffi::CStr::from_ptr(pointer) }.to_owned());
        }
        if !extensions
            .iter()
            .any(|name| name.as_c_str() == ash::khr::surface::NAME)
        {
            return None;
        }
        let context = Arc::new(Context::with_extensions(&extensions, true)?);
        Self::from_context(context, host.render_scale)
    }

    /// Allocate a pool in the lazy arena and return its guest GPU base, or None.
    pub fn allocate_pool(&mut self, key: u64, storage: u64, size: u64) -> Option<u64> {
        if self.global_memory.is_none() {
            self.global_memory = Some(GlobalMemory::new(
                &self.context,
                crate::global_memory::ARENA_SIZE,
            )?);
        }
        self.global_memory
            .as_mut()?
            .allocate_pool(key, storage, size)
    }

    /// Release a pool and its dependent textures; return whether it existed.
    pub fn release_pool(&mut self, key: u64) -> bool {
        let textures: Vec<_> = self
            .bindings
            .iter()
            .filter_map(|(&texture, &(pool, _, _))| (pool == key).then_some(texture))
            .collect();
        for texture in textures {
            self.release_texture(texture);
        }
        self.global_memory
            .as_mut()
            .is_some_and(|memory| memory.release_pool(key))
    }

    pub(crate) fn resolved_image(
        &self,
        d: &crate::api::TextureDescription,
    ) -> Option<image_layout::ResolvedImage> {
        self.image_contract.resolve(d)
    }
    pub(crate) fn has_image_contract(&self) -> bool {
        !self.image_contract.rules.is_empty()
    }
    pub(crate) fn texture_storage_size(&self, d: &crate::api::TextureDescription) -> Option<usize> {
        let r = self.image_contract.resolve(d)?;
        Some(match r.storage {
            image_layout::Storage::Linear => r.packing.linear_size(),
            image_layout::Storage::Tiled(_) => r.packing.tiled_size(),
        })
    }

    /// Install validated image rules before any arena image bindings exist.
    pub fn set_image_contract(&mut self, contract: image_layout::ImageContract) -> bool {
        if contract.validate().is_err() || !self.bindings.is_empty() {
            return false;
        }
        self.image_contract = contract;
        true
    }

    /// Return the shared execution context owned by this backend.
    pub fn context(&self) -> &Arc<Context> {
        &self.context
    }

    /// Create or reuse a color or depth image for the key; return false on failure.
    pub fn ensure(&mut self, key: u64, width: u64, height: u64, depth: bool) -> bool {
        self.images.ensure(key, width, height, depth)
    }

    /// The base level uses novena's existing four-byte, tightly packed storage
    /// choice. Guest format integers remain uninterpreted.
    pub(crate) fn ensure_texture(
        &mut self,
        key: u64,
        description: &crate::api::TextureDescription,
        depth: bool,
    ) -> bool {
        if !self.image_contract.rules.is_empty() {
            let Some(resolved) = self.image_contract.resolve(description) else {
                return false;
            };
            if depth && resolved.descriptor.format != vk::Format::D32_SFLOAT {
                return false;
            }
            let bytes = match resolved.storage {
                image_layout::Storage::Linear => resolved.packing.linear_size(),
                image_layout::Storage::Tiled(_) => resolved.packing.tiled_size(),
            };
            if description.pool == 0
                || self
                    .global_memory
                    .as_ref()
                    .and_then(|m| m.image_region(description.pool, description.pool_offset, bytes))
                    .is_none()
            {
                return false;
            }
            if !self.images.ensure_descriptor(key, &resolved.descriptor) {
                return false;
            }
            self.bindings
                .insert(key, (description.pool, description.pool_offset, bytes));
            if matches!(resolved.storage, image_layout::Storage::Tiled(_)) {
                self.tiled.insert(key, resolved.packing);
            } else {
                self.tiled.remove(&key);
            }
            return true;
        }
        let width = description.width.max(1);
        let height = description.height.max(1);
        if description.depth > 1
            || (description.stride != 0 && description.stride != width.saturating_mul(4))
        {
            return false;
        }
        let Some(bytes) = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| usize::try_from(n).ok())
        else {
            return false;
        };
        if description.pool != 0 {
            let Some(memory) = self.global_memory.as_ref() else {
                return false;
            };
            if memory
                .image_region(description.pool, description.pool_offset, bytes)
                .is_none()
            {
                return false;
            }
        }
        if !self.ensure(key, width, height, depth) {
            return false;
        }
        if description.pool != 0 {
            self.bindings
                .insert(key, (description.pool, description.pool_offset, bytes));
        } else {
            self.bindings.remove(&key);
        }
        true
    }

    pub(crate) fn ensure_depth_stencil(
        &mut self,
        key: u64,
        description: &crate::api::TextureDescription,
    ) -> bool {
        let Some(bytes) = description
            .width
            .checked_mul(description.height)
            .and_then(|n| n.checked_mul(5))
            .and_then(|n| usize::try_from(n).ok())
        else {
            return false;
        };
        if description.width == 0
            || description.height == 0
            || description.pool == 0
            || description.stride != 0
        {
            return false;
        }
        if self
            .global_memory
            .as_ref()
            .and_then(|m| m.image_region(description.pool, description.pool_offset, bytes))
            .is_none()
        {
            return false;
        }
        if !self.images.ensure_format(
            key,
            description.width,
            description.height,
            vk::Format::D32_SFLOAT_S8_UINT,
        ) {
            return false;
        }
        self.bindings
            .insert(key, (description.pool, description.pool_offset, bytes));
        true
    }

    fn sync_texture(&mut self, key: u64, load: bool) -> Option<()> {
        if let Some(&(pool, offset, bytes)) = self.bindings.get(&key) {
            let (buffer, offset) = self
                .global_memory
                .as_ref()?
                .image_region(pool, offset, bytes)?;
            if let Some(packing) = self.tiled.get(&key) {
                let address = unsafe {
                    self.context.device.get_buffer_device_address(
                        &vk::BufferDeviceAddressInfo::default().buffer(buffer),
                    )
                }
                .checked_add(offset)?;
                self.images.tiled_transfer(key, address, packing, load)?;
            } else {
                self.images.arena_transfer(key, buffer, offset, load)?;
            }
        }
        Some(())
    }

    pub(crate) fn sampled_texture(
        &mut self,
        key: u64,
        description: &crate::api::TextureDescription,
    ) -> Option<vk::ImageView> {
        if !self.ensure_texture(key, description, false) {
            return None;
        }
        self.sync_texture(key, true)?;
        if self.image_contract.swizzles.is_empty() {
            self.images.sampled(key)
        } else {
            let components = self.image_contract.resolve(description)?.components;
            self.images.sampled_with_components(key, components)
        }
    }

    pub(crate) fn sampler(
        &mut self,
        pool: u64,
        id: u32,
        key: textures::SamplerKey,
    ) -> Option<vk::Sampler> {
        if self.samplers.get(&(pool, id)).is_none_or(|s| s.key != key) {
            self.samplers
                .insert((pool, id), textures::Sampler::new(&self.context, key)?);
        }
        Some(self.samplers.get(&(pool, id))?.handle)
    }

    /// Create a sampler through the recorded-state mapping and device checks.
    pub fn mapped_sampler(
        &mut self,
        pool: u64,
        id: u32,
        description: &textures::SamplerDescription,
        contract: &textures::TextureContract,
    ) -> Option<vk::Sampler> {
        contract.validate().ok()?;
        let key = textures::SamplerKey::new(description, contract)?;
        self.sampler(pool, id, key)
    }

    /// Allocate or reuse a typed host image. Compressed payloads remain opaque.
    pub fn ensure_image(&mut self, key: u64, descriptor: &image_layout::ImageDescriptor) -> bool {
        self.images.ensure_descriptor(key, descriptor)
    }

    /// Return a whole-resource view after making transfers visible to shaders.
    pub fn sampled_image(&mut self, key: u64) -> Option<vk::ImageView> {
        self.images.sampled(key)
    }

    /// Return a sampled view with explicit host component selection.
    pub fn sampled_image_with_components(
        &mut self,
        key: u64,
        components: vk::ComponentMapping,
    ) -> Option<vk::ImageView> {
        if [components.r, components.g, components.b, components.a]
            .iter()
            .any(|c| !(0..=6).contains(&c.as_raw()))
        {
            return None;
        }
        self.images.sampled_with_components(key, components)
    }

    pub(crate) fn has_swizzle_contract(&self) -> bool {
        !self.image_contract.swizzles.is_empty()
    }

    /// Upload a whole typed image in packed-linear order; return false on failure.
    pub fn upload_image(&mut self, key: u64, bytes: &[u8]) -> bool {
        self.images.upload(key, bytes).is_some()
    }

    /// Transfer tiled pool storage directly through its device-visible arena buffer.
    pub fn load_tiled(
        &mut self,
        key: u64,
        pool: u64,
        offset: u64,
        packing: &crate::tiling::Layout,
    ) -> bool {
        self.transfer_tiled(key, pool, offset, packing, true)
            .is_some()
    }
    /// Write an image into tiled arena storage; return false for invalid ranges or transfers.
    pub fn store_tiled(
        &mut self,
        key: u64,
        pool: u64,
        offset: u64,
        packing: &crate::tiling::Layout,
    ) -> bool {
        self.transfer_tiled(key, pool, offset, packing, false)
            .is_some()
    }
    /// Batch bounded transfers in one reusable command recording.
    pub fn load_tiled_batch(
        &mut self,
        resources: &[(u64, u64, u64, &crate::tiling::Layout)],
    ) -> bool {
        self.transfer_tiled_batch(resources, true).is_some()
    }
    /// Store up to 256 image transfers with shared scratch and submission.
    pub fn store_tiled_batch(
        &mut self,
        resources: &[(u64, u64, u64, &crate::tiling::Layout)],
    ) -> bool {
        self.transfer_tiled_batch(resources, false).is_some()
    }
    fn transfer_tiled_batch(
        &mut self,
        resources: &[(u64, u64, u64, &crate::tiling::Layout)],
        load: bool,
    ) -> Option<()> {
        if resources.is_empty() || resources.len() > 256 {
            return None;
        }
        let memory = self.global_memory.as_ref()?;
        let mut ranges = Vec::with_capacity(resources.len());
        for &(key, pool, offset, packing) in resources {
            let (buffer, at) = memory.image_region(pool, offset, packing.tiled_size())?;
            let address = unsafe {
                self.context.device.get_buffer_device_address(
                    &vk::BufferDeviceAddressInfo::default().buffer(buffer),
                )
            }
            .checked_add(at)?;
            ranges.push((key, address, packing));
        }
        self.images.tiled_batch(&ranges, load)
    }

    fn transfer_tiled(
        &mut self,
        key: u64,
        pool: u64,
        offset: u64,
        packing: &crate::tiling::Layout,
        load: bool,
    ) -> Option<()> {
        let (buffer, at) =
            self.global_memory
                .as_ref()?
                .image_region(pool, offset, packing.tiled_size())?;
        if !at.is_multiple_of(4) {
            return None;
        }
        let address = unsafe {
            self.context
                .device
                .get_buffer_device_address(&vk::BufferDeviceAddressInfo::default().buffer(buffer))
        }
        .checked_add(at)?;
        self.images.tiled_transfer(key, address, packing, load)
    }

    /// Copy packed linear arena images in one submission, without conversion.
    pub fn load_linear_batch(
        &mut self,
        resources: &[(u64, u64, u64, &crate::tiling::Layout)],
    ) -> bool {
        self.transfer_linear_batch(resources, true).is_some()
    }

    /// Copy images to packed linear arena storage in one submission.
    pub fn store_linear_batch(
        &mut self,
        resources: &[(u64, u64, u64, &crate::tiling::Layout)],
    ) -> bool {
        self.transfer_linear_batch(resources, false).is_some()
    }

    fn transfer_linear_batch(
        &mut self,
        resources: &[(u64, u64, u64, &crate::tiling::Layout)],
        load: bool,
    ) -> Option<()> {
        if resources.is_empty() || resources.len() > 256 {
            return None;
        }
        let memory = self.global_memory.as_ref()?;
        let mut ranges = Vec::with_capacity(resources.len());
        for &(key, pool, offset, packing) in resources {
            let (buffer, at) = memory.image_region(pool, offset, packing.linear_size())?;
            ranges.push((key, buffer, at, packing));
        }
        self.images.linear_batch(&ranges, load)
    }

    /// Complete all transfers before CPU access to shared arena storage.
    pub fn wait_transfers(&self) -> bool {
        self.images.wait().is_some()
    }

    /// Remove the image and arena binding for a texture key.
    pub fn release_texture(&mut self, key: u64) {
        self.images.remove(key);
        self.bindings.remove(&key);
        self.tiled.remove(&key);
    }

    /// Clear selected RGBA channels; mask bits zero through three select channels.
    pub fn clear_color(&mut self, key: u64, color: [f32; 4], mask: u32) -> bool {
        (|| {
            self.sync_texture(key, true)?;
            self.images.clear_color(key, color, mask)?;
            self.sync_texture(key, false)
        })()
        .is_some()
    }

    /// Clear a depth image; the current depth-only path does not use stencil.
    pub fn clear_depth(&mut self, key: u64, depth: f32, stencil: u32) -> bool {
        if stencil != 0 {
            return false;
        }
        (|| {
            self.sync_texture(key, true)?;
            self.images.clear_depth(key, depth)?;
            self.sync_texture(key, false)
        })()
        .is_some()
    }

    pub(crate) fn draw(
        &mut self,
        textures: &[u64],
        depth: Option<u64>,
        pipeline: &graphics::GraphicsPipeline,
        draw: &graphics::Draw,
    ) -> Option<()> {
        for &texture in textures {
            self.sync_texture(texture, true)?;
        }
        if let Some(depth) = depth {
            self.sync_texture(depth, true)?;
        }
        self.images.draw(
            textures,
            depth,
            pipeline,
            self.global_memory.as_ref()?,
            draw,
        )?;
        for &texture in textures {
            self.sync_texture(texture, false)?;
        }
        if let Some(depth) = depth {
            self.sync_texture(depth, false)?;
        }
        Some(())
    }

    /// Return unscaled RGBA image bytes and dimensions, or None on failure.
    pub fn readback(&mut self, key: u64) -> Option<(u32, u32, Vec<u8>)> {
        self.sync_texture(key, true)?;
        self.images.readback(key)
    }

    /// Upload tightly packed four-byte color texels into the keyed image.
    pub fn upload(&mut self, key: u64, data: &[u8], width: u64, height: u64) -> bool {
        (|| {
            if !self.ensure(key, width, height, false) {
                return None;
            }
            self.images.upload(key, data)?;
            self.sync_texture(key, false)
        })()
        .is_some()
    }

    pub(crate) fn copy_from_arena(
        &mut self,
        key: u64,
        pool: u64,
        offset: u64,
        size: usize,
    ) -> bool {
        (|| {
            let (buffer, offset) = self
                .global_memory
                .as_ref()?
                .image_region(pool, offset, size)?;
            self.images.arena_transfer(key, buffer, offset, true)?;
            self.sync_texture(key, false)
        })()
        .is_some()
    }

    /// Copy equal-format image regions; return false for unsupported or invalid regions.
    pub fn copy_region(
        &mut self,
        destination: u64,
        source: u64,
        from: image_layout::ImageRegion,
        to: image_layout::ImageRegion,
    ) -> bool {
        (|| {
            self.sync_texture(source, true)?;
            self.sync_texture(destination, true)?;
            self.images.copy_region(destination, source, from, to)?;
            self.sync_texture(destination, false)
        })()
        .is_some()
    }
    /// Blit compatible image regions with the selected filter; return false on failure.
    pub fn blit_region(
        &mut self,
        destination: u64,
        source: u64,
        from: image_layout::BlitRegion,
        to: image_layout::BlitRegion,
        filter: vk::Filter,
    ) -> bool {
        (|| {
            self.sync_texture(source, true)?;
            self.sync_texture(destination, true)?;
            self.images
                .blit_region(destination, source, from, to, filter)?;
            self.sync_texture(destination, false)
        })()
        .is_some()
    }

    /// Copy a whole image between existing compatible images; return false on failure.
    pub fn copy(&mut self, destination: u64, source: u64) -> bool {
        (|| {
            self.sync_texture(source, true)?;
            self.images.copy(destination, source)?;
            self.sync_texture(destination, false)
        })()
        .is_some()
    }

    pub(crate) fn open_window(
        &mut self,
        host: &crate::Host,
        key: u64,
        native_window: u64,
    ) -> Option<()> {
        use ash::vk::Handle;
        if host.vulkan.is_null() {
            return Some(());
        }
        if !self.context.swapchain || self.windows.contains_key(&key) {
            return None;
        }
        let surface_host = unsafe { &*host.vulkan };
        let raw = unsafe {
            (surface_host.create_surface)(
                host.user,
                self.context.instance.handle().as_raw(),
                key,
                native_window,
            )
        };
        if raw == 0 {
            return None;
        }
        let window = Window::new(&self.context, vk::SurfaceKHR::from_raw(raw))?;
        self.windows.insert(key, window);
        Some(())
    }

    pub(crate) fn close_window(&mut self, key: u64) {
        self.windows.remove(&key);
    }

    pub(crate) fn present_window(
        &mut self,
        host: &crate::Host,
        window: u64,
        texture: u64,
        interval: u32,
    ) -> Option<()> {
        let mut extent = vk::Extent2D::default();
        let surface_host = unsafe { &*host.vulkan };
        unsafe {
            (surface_host.drawable_size)(host.user, window, &mut extent.width, &mut extent.height);
        }
        self.sync_texture(texture, true)?;
        let source = self.images.source(texture)?;
        if source.format != vk::Format::R8G8B8A8_UNORM {
            return None;
        }
        self.windows
            .get_mut(&window)?
            .present(source, extent, interval)
    }

    /// Offscreen presentation uses the same blit and barriers as a swapchain.
    /// Returned bytes use the destination format, including BGRA order and sRGB encoding.
    pub fn present_offscreen(
        &mut self,
        texture: u64,
        format: vk::Format,
        width: u32,
        height: u32,
    ) -> Option<(u32, u32, Vec<u8>)> {
        if !matches!(
            format,
            vk::Format::R8G8B8A8_UNORM
                | vk::Format::B8G8R8A8_UNORM
                | vk::Format::R8G8B8A8_SRGB
                | vk::Format::B8G8R8A8_SRGB
        ) {
            return None;
        }
        self.sync_texture(texture, true)?;
        let source = self.images.source(texture)?;
        if source.format != vk::Format::R8G8B8A8_UNORM {
            return None;
        }
        if !self.offscreen.as_ref().is_some_and(|image| {
            image.info.format == format
                && image.info.extent.width == width
                && image.info.extent.height == height
        }) {
            self.offscreen = Some(Image::new(&self.context, width, height, format)?);
        }
        let target = self.offscreen.as_mut()?;
        self.images.blit_offscreen(source, target)?;
        Some((width, height, self.images.read_image(target)?))
    }

    pub(crate) fn present_callback(
        &mut self,
        texture: u64,
        width: u64,
        height: u64,
    ) -> Option<(u32, u32, Vec<u8>)> {
        let width = (width.max(1) as f32 * self.scale).round() as u32;
        let height = (height.max(1) as f32 * self.scale).round() as u32;
        self.present_offscreen(texture, vk::Format::R8G8B8A8_UNORM, width, height)
    }

    pub(crate) fn finish(&self) -> bool {
        unsafe { self.context.device.device_wait_idle().is_ok() }
    }
}

impl Context {
    /// Create a module after checking the optional global memory features.
    /// The caller still validates SPIR-V and owns the returned Vulkan module.
    pub fn create_global_shader_module(&self, words: &[u32]) -> Result<vk::ShaderModule, String> {
        self.shader_features
            .validate_global_shader(words)
            .map_err(str::to_owned)?;
        // SAFETY: the feature checks ran above; Vulkan consumes the supplied words.
        unsafe {
            self.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(words), None)
        }
        .map_err(|error| format!("create global shader module: {error:?}"))
    }
    /// Create a context, preferring a discrete GPU and then any usable GPU.
    pub fn new() -> Option<Self> {
        Self::with_extensions(&[], false)
    }

    fn with_extensions(extensions: &[CString], swapchain: bool) -> Option<Self> {
        // SAFETY: loading the system Vulkan loader is the boundary of this
        // optional backend; ash validates the returned function table.
        let entry = unsafe { Entry::load().ok()? };
        let app = CString::new("novena").ok()?;
        let info = vk::ApplicationInfo::default()
            .application_name(&app)
            .engine_name(&app)
            .api_version(vk::API_VERSION_1_2);
        let pointers: Vec<_> = extensions.iter().map(|name| name.as_ptr()).collect();
        let create = vk::InstanceCreateInfo::default()
            .application_info(&info)
            .enabled_extension_names(&pointers);
        // SAFETY: `create` points only to local, immutable Vulkan structs.
        let instance = unsafe { entry.create_instance(&create, None).ok()? };
        // SAFETY: the instance is live and owns this enumeration.
        let result = Self::create_device(&instance, swapchain);
        let Some((pick, device, family, shader_features)) = result else {
            // SAFETY: no device was created and the instance is owned here.
            unsafe { instance.destroy_instance(None) };
            return None;
        };
        // SAFETY: queue zero was requested by create_device.
        let queue = unsafe { device.get_device_queue(family, 0) };
        let queue_families = unsafe { instance.get_physical_device_queue_family_properties(pick) }
            .iter()
            .enumerate()
            .filter_map(|(i, family)| (family.queue_count > 0).then_some(i as u32))
            .collect();
        Some(Self {
            entry,
            instance,
            physical_device: pick,
            device,
            queue,
            queue_family: family,
            shader_features,
            queue_families,
            swapchain,
        })
    }

    fn create_device(
        instance: &Instance,
        swapchain: bool,
    ) -> Option<(vk::PhysicalDevice, Device, u32, ShaderFeatures)> {
        // SAFETY: instance is live for all enumeration and feature queries below.
        let mut devices = unsafe { instance.enumerate_physical_devices().ok()? };
        devices.sort_by_key(|&device| unsafe {
            instance.get_physical_device_properties(device).device_type
                != vk::PhysicalDeviceType::DISCRETE_GPU
        });
        for pick in devices {
            if swapchain {
                let supported =
                    unsafe { instance.enumerate_device_extension_properties(pick) }.ok()?;
                if !supported.iter().any(|extension| {
                    (unsafe { std::ffi::CStr::from_ptr(extension.extension_name.as_ptr()) })
                        == ash::khr::swapchain::NAME
                }) {
                    continue;
                }
            }
            if unsafe { instance.get_physical_device_properties(pick).api_version }
                < vk::API_VERSION_1_2
            {
                continue;
            }
            let families = unsafe { instance.get_physical_device_queue_family_properties(pick) };
            let Some(family) = families.iter().position(|f| {
                f.queue_count > 0
                    && f.queue_flags
                        .contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE)
            }) else {
                continue;
            };
            let family = family as u32;
            let mut v11 = vk::PhysicalDeviceVulkan11Features::default();
            let mut v12 = vk::PhysicalDeviceVulkan12Features::default();
            let mut features = vk::PhysicalDeviceFeatures2::default()
                .push_next(&mut v11)
                .push_next(&mut v12);
            unsafe { instance.get_physical_device_features2(pick, &mut features) };
            let core = features.features;
            if core.shader_int64 == vk::FALSE || v12.buffer_device_address == vk::FALSE {
                continue;
            }
            let shader_features = ShaderFeatures {
                storage_buffer8_bit_access: v12.storage_buffer8_bit_access != vk::FALSE,
                storage_buffer16_bit_access: v11.storage_buffer16_bit_access != vk::FALSE,
                shader_int8: v12.shader_int8 != vk::FALSE,
                shader_int16: core.shader_int16 != vk::FALSE,
            };
            let enabled = vk::PhysicalDeviceFeatures::default()
                .independent_blend(core.independent_blend != 0)
                .sampler_anisotropy(core.sampler_anisotropy != 0)
                .shader_int64(true)
                .image_cube_array(core.image_cube_array != 0)
                .texture_compression_bc(core.texture_compression_bc != 0)
                .texture_compression_astc_ldr(core.texture_compression_astc_ldr != 0)
                .shader_int16(shader_features.shader_int16)
                .fill_mode_non_solid(core.fill_mode_non_solid != 0)
                .depth_bias_clamp(core.depth_bias_clamp != 0);
            let mut enabled11 = vk::PhysicalDeviceVulkan11Features::default()
                .storage_buffer16_bit_access(shader_features.storage_buffer16_bit_access);
            let mut enabled12 = vk::PhysicalDeviceVulkan12Features::default()
                .buffer_device_address(true)
                .storage_buffer8_bit_access(shader_features.storage_buffer8_bit_access)
                .shader_int8(shader_features.shader_int8);
            let priority = [1.0_f32];
            let queue_info: Vec<_> = families
                .iter()
                .enumerate()
                .filter(|(_, properties)| properties.queue_count > 0)
                .map(|(i, _)| {
                    vk::DeviceQueueCreateInfo::default()
                        .queue_family_index(i as u32)
                        .queue_priorities(&priority)
                })
                .collect();
            let extensions: Vec<_> = if swapchain {
                vec![ash::khr::swapchain::NAME.as_ptr()]
            } else {
                Vec::new()
            };
            let device_info = vk::DeviceCreateInfo::default()
                .queue_create_infos(&queue_info)
                .enabled_extension_names(&extensions)
                .enabled_features(&enabled)
                .push_next(&mut enabled11)
                .push_next(&mut enabled12);
            // SAFETY: the selected family supports graphics and the create info is valid.
            if let Ok(device) = unsafe { instance.create_device(pick, &device_info, None) } {
                return Some((pick, device, family, shader_features));
            }
        }
        None
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

#[cfg(test)]
mod tests {
    use super::{Backend, ShaderFeatures};

    #[test]
    fn rejects_unavailable_narrow_capabilities_before_module_creation() {
        let supported = ShaderFeatures {
            storage_buffer8_bit_access: true,
            storage_buffer16_bit_access: true,
            shader_int8: true,
            shader_int16: true,
        };
        for capability in [39, 22, 4448, 4433] {
            let words = [0x0723_0203, 0x10300, 0, 1, 0, (2 << 16) | 17, capability];
            assert!(ShaderFeatures::default()
                .validate_global_shader(&words)
                .is_err());
            assert!(supported.validate_global_shader(&words).is_ok());
        }
        assert!(supported.validate_global_shader(&[]).is_err());
        assert!(supported
            .validate_global_shader(&[0x0723_0203, 0x10300, 0, 1, 0, 0])
            .is_err());
        assert!(supported
            .validate_global_shader(&[0x0723_0203, 0x10300, 0, 1, 0, (3 << 16) | 17, 39])
            .is_err());
    }

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
        let (width, height, pixels) = backend
            .present_callback(1, 2, 1)
            .expect("Vulkan presentation");
        assert_eq!((width, height), (4, 2));
        assert_eq!(&pixels[..4], &[0, 0, 255, 255]);
    }
}
