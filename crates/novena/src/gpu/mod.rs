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
mod images;
mod memory;
mod pipeline_disk;
mod pipeline_workers;
mod present;
use images::{Image, Images};
use present::Window;
pub mod pipelines;
pub use memory::GlobalMemory;

#[derive(Clone, Copy, Debug, Default)]
pub struct ShaderFeatures {
    pub storage_buffer8_bit_access: bool,
    pub storage_buffer16_bit_access: bool,
    pub shader_int8: bool,
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
    pub entry: Entry,
    pub instance: Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
    pub shader_features: ShaderFeatures,
    pub queue_families: Vec<u32>,
    swapchain: bool,
}

pub struct Backend {
    pub(crate) first_draw: Option<graphics::FirstDrawContract>,
    pub(crate) graphics: graphics::GraphicsPipelines,
    windows: HashMap<u64, Window>,
    offscreen: Option<Image>,
    images: Images,
    bindings: HashMap<u64, (u64, u64, usize)>,
    pub global_memory: Option<GlobalMemory>,
    context: Arc<Context>,
    scale: f32,
}

impl Backend {
    pub fn new(scale: f32) -> Option<Self> {
        Self::from_context(Arc::new(Context::new()?), scale)
    }

    fn from_context(context: Arc<Context>, scale: f32) -> Option<Self> {
        let images = Images::new(&context)?;
        Some(Self {
            first_draw: None,
            graphics: graphics::GraphicsPipelines::new(&context)?,
            windows: HashMap::new(),
            offscreen: None,
            images,
            bindings: HashMap::new(),
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

    pub fn context(&self) -> &Arc<Context> {
        &self.context
    }

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

    fn sync_texture(&mut self, key: u64, load: bool) -> Option<()> {
        if let Some(&(pool, offset, bytes)) = self.bindings.get(&key) {
            let (buffer, offset) = self
                .global_memory
                .as_ref()?
                .image_region(pool, offset, bytes)?;
            self.images.arena_transfer(key, buffer, offset, load)?;
        }
        Some(())
    }

    pub fn release_texture(&mut self, key: u64) {
        self.images.remove(key);
        self.bindings.remove(&key);
    }

    pub fn clear_color(&mut self, key: u64, color: [f32; 4], mask: u32) -> bool {
        (|| {
            self.sync_texture(key, true)?;
            self.images.clear_color(key, color, mask)?;
            self.sync_texture(key, false)
        })()
        .is_some()
    }

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
        texture: u64,
        pipeline: &graphics::GraphicsPipeline,
        draw: &graphics::Draw,
    ) -> Option<()> {
        self.sync_texture(texture, true)?;
        self.images
            .draw(texture, pipeline, self.global_memory.as_ref()?, draw)?;
        self.sync_texture(texture, false)
    }

    pub fn readback(&mut self, key: u64) -> Option<(u32, u32, Vec<u8>)> {
        self.sync_texture(key, true)?;
        self.images.readback(key)
    }

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
                .shader_int64(true)
                .shader_int16(shader_features.shader_int16);
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
