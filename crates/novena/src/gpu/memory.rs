//! Vulkan backing for the flat address space. Provenance: 0022-flat-global-memory.

use super::{find_memory_type, Context};
use crate::global_memory::{
    AddressMap, Allocator, GUEST_BASE, PUSH_GLOBAL_DELTA_OFFSET, PUSH_GLOBAL_DELTA_SIZE,
};
use ash::{vk, Device};
use std::sync::Arc;

/// One buffer and one stable device address for every live guest pool.
/// Methods require exclusive access, including across queue submission and readback.
pub struct GlobalMemory {
    context: Arc<Context>,
    device: Device,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    addresses: AddressMap,
    allocator: Allocator,
}

impl GlobalMemory {
    pub fn new(context: &Arc<Context>, size: u64) -> Option<Self> {
        if size == 0
            || !size.is_multiple_of(16)
            || GUEST_BASE.checked_add(size)? > u64::from(u32::MAX) + 1
        {
            return None;
        }
        let info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(
                vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                    | vk::BufferUsageFlags::STORAGE_BUFFER
                    | vk::BufferUsageFlags::UNIFORM_BUFFER
                    | vk::BufferUsageFlags::VERTEX_BUFFER
                    | vk::BufferUsageFlags::INDEX_BUFFER
                    | vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: the device has bufferDeviceAddress enabled and size is nonzero.
        let buffer = unsafe { context.device.create_buffer(&info, None).ok()? };
        let requirements = unsafe { context.device.get_buffer_memory_requirements(buffer) };
        let Some(ty) = find_memory_type(
            context,
            requirements.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        ) else {
            unsafe { context.device.destroy_buffer(buffer, None) };
            return None;
        };
        let mut flags =
            vk::MemoryAllocateFlagsInfo::default().flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(ty)
            .push_next(&mut flags);
        // SAFETY: allocation uses the buffer requirements and device address flag.
        let Ok(memory) = (unsafe { context.device.allocate_memory(&alloc, None) }) else {
            unsafe { context.device.destroy_buffer(buffer, None) };
            return None;
        };
        let addresses = (|| {
            unsafe { context.device.bind_buffer_memory(buffer, memory, 0).ok()? };
            let host = unsafe {
                context.device.get_buffer_device_address(
                    &vk::BufferDeviceAddressInfo::default().buffer(buffer),
                )
            };
            AddressMap::new(GUEST_BASE, host, size)
        })();
        let Some(addresses) = addresses else {
            // SAFETY: no work was submitted using the buffer.
            unsafe {
                context.device.destroy_buffer(buffer, None);
                context.device.free_memory(memory, None);
            }
            return None;
        };
        Some(Self {
            context: Arc::clone(context),
            device: context.device.clone(),
            buffer,
            memory,
            addresses,
            allocator: Allocator::new(size),
        })
    }

    pub fn addresses(&self) -> AddressMap {
        self.addresses
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    /// A bounded uniform descriptor for a live pool. Evidence: provenance 0025.
    /// The caller retains this arena and pool until descriptor users finish.
    pub fn uniform_buffer_info(
        &self,
        key: u64,
        offset: u64,
        size: u64,
    ) -> Option<vk::DescriptorBufferInfo> {
        let at = self.pool_offset(key, offset, usize::try_from(size).ok()?)?;
        // SAFETY: querying immutable properties of this context's physical device.
        let limits = unsafe {
            self.context
                .instance
                .get_physical_device_properties(self.context.physical_device)
        }
        .limits;
        if size == 0
            || size > u64::from(limits.max_uniform_buffer_range)
            || !at.is_multiple_of(limits.min_uniform_buffer_offset_alignment)
        {
            return None;
        }
        Some(
            vk::DescriptorBufferInfo::default()
                .buffer(self.buffer)
                .offset(at)
                .range(size),
        )
    }

    /// Storage fallback for constant banks with a device-aligned arena range.
    pub(crate) fn storage_buffer_info(
        &self,
        key: u64,
        offset: u64,
        size: u64,
    ) -> Option<vk::DescriptorBufferInfo> {
        let at = self.pool_offset(key, offset, usize::try_from(size).ok()?)?;
        let limits = unsafe {
            self.context
                .instance
                .get_physical_device_properties(self.context.physical_device)
        }
        .limits;
        if size == 0
            || size > u64::from(limits.max_storage_buffer_range)
            || !at.is_multiple_of(limits.min_storage_buffer_offset_alignment)
        {
            return None;
        }
        Some(
            vk::DescriptorBufferInfo::default()
                .buffer(self.buffer)
                .offset(at)
                .range(size),
        )
    }

    /// Canonical tightly packed base-level texels in a live arena pool.
    pub(crate) fn image_region(
        &self,
        key: u64,
        offset: u64,
        size: usize,
    ) -> Option<(vk::Buffer, u64)> {
        let (buffer, at) = self.buffer_region(key, offset, size)?;
        // Four-byte colour and depth texels require four-byte copy offsets.
        at.is_multiple_of(4).then_some((buffer, at))
    }

    /// A bounded nonempty slice of the canonical arena. Callers check use-specific alignment.
    pub(crate) fn buffer_region(
        &self,
        key: u64,
        offset: u64,
        size: usize,
    ) -> Option<(vk::Buffer, u64)> {
        let at = self.pool_offset(key, offset, size)?;
        (size > 0).then_some((self.buffer, at))
    }

    pub fn contains_pool(&self, key: u64) -> bool {
        self.allocator.pools.contains_key(&key)
    }

    pub fn allocate_pool(&mut self, key: u64, storage: u64, size: u64) -> Option<u64> {
        let aliased = self.allocator.pools.values().any(|pool| {
            storage >= pool.storage
                && storage
                    .checked_add(size)
                    .is_some_and(|end| end <= pool.storage + pool.size)
        });
        let pool = self.allocator.allocate(key, storage, size)?;
        if !aliased {
            let reserved = (size + 15) & !15;
            // SAFETY: this new block is within the buffer. New or recycled bytes
            // and the complete-word padding start zeroed before use.
            let cleared = unsafe {
                self.device
                    .map_memory(self.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                    .map(|mapped| {
                        std::ptr::write_bytes(
                            mapped.cast::<u8>().add(pool.offset as usize),
                            0,
                            reserved as usize,
                        );
                        self.device.unmap_memory(self.memory);
                    })
            };
            if cleared.is_err() {
                self.allocator.release(key);
                return None;
            }
        }
        self.addresses.guest(pool.offset)
    }

    pub fn release_pool(&mut self, key: u64) -> bool {
        // SAFETY: this backend serializes queue use. Waiting also protects callers
        // that submitted work directly through Context before finalizing a pool.
        if unsafe { self.device.device_wait_idle() }.is_err() {
            return false;
        }
        self.allocator.release(key)
    }

    fn pool_offset(&self, key: u64, offset: u64, size: usize) -> Option<u64> {
        let pool = self.allocator.pools.get(&key)?;
        (offset.checked_add(size as u64)? <= pool.size).then(|| pool.offset + offset)
    }

    pub fn write_pool(&mut self, key: u64, offset: u64, bytes: &[u8]) -> Option<()> {
        self.write_at(self.pool_offset(key, offset, bytes.len())?, bytes)
    }

    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> Option<()> {
        if bytes.is_empty() {
            return Some(());
        }
        // SAFETY: the range is inside the mapped buffer, coherent memory needs no
        // flush, and exclusive access prevents simultaneous host or queue writes.
        unsafe {
            self.device.device_wait_idle().ok()?;
            let mapped = self
                .device
                .map_memory(self.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                .ok()?;
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                mapped.cast::<u8>().add(offset as usize),
                bytes.len(),
            );
            self.device.unmap_memory(self.memory);
        }
        Some(())
    }

    /// Read coherent bytes after completion. Submitted shader writes must include
    /// a shader-write to host-read memory barrier before this method is called.
    pub fn read_pool(&mut self, key: u64, offset: u64, bytes: &mut [u8]) -> Option<()> {
        let offset = self.pool_offset(key, offset, bytes.len())?;
        if bytes.is_empty() {
            return Some(());
        }
        // SAFETY: waiting completes submitted writes; the range is inside coherent
        // host-visible memory and the output slice is valid for its length.
        unsafe {
            self.device.device_wait_idle().ok()?;
            let mapped = self
                .device
                .map_memory(self.memory, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
                .ok()?;
            std::ptr::copy_nonoverlapping(
                mapped.cast::<u8>().add(offset as usize),
                bytes.as_mut_ptr(),
                bytes.len(),
            );
            self.device.unmap_memory(self.memory);
        }
        Some(())
    }

    /// Copy through the host callbacks in bounded chunks at submission boundaries.
    pub fn upload(&mut self, mut read: impl FnMut(u64, &mut [u8]) -> bool) -> bool {
        let pools: Vec<_> = self
            .allocator
            .pools
            .iter()
            .map(|(&key, &pool)| (key, pool))
            .collect();
        let mut bytes = vec![0; 64 * 1024];
        for (key, pool) in pools {
            let mut offset = 0;
            while offset < pool.size {
                let length = (pool.size - offset).min(bytes.len() as u64) as usize;
                if !read(pool.storage + offset, &mut bytes[..length])
                    || self.write_pool(key, offset, &bytes[..length]).is_none()
                {
                    return false;
                }
                offset += length as u64;
            }
        }
        true
    }

    /// As with read_pool, shader writes require a host-read barrier in the submission.
    pub fn download(&mut self, mut write: impl FnMut(u64, &[u8]) -> bool) -> bool {
        let pools: Vec<_> = self
            .allocator
            .pools
            .iter()
            .map(|(&key, &pool)| (key, pool))
            .collect();
        let mut bytes = vec![0; 64 * 1024];
        for (key, pool) in pools {
            let mut offset = 0;
            while offset < pool.size {
                let length = (pool.size - offset).min(bytes.len() as u64) as usize;
                if self.read_pool(key, offset, &mut bytes[..length]).is_none()
                    || !write(pool.storage + offset, &bytes[..length])
                {
                    return false;
                }
                offset += length as u64;
            }
        }
        true
    }

    pub fn push_constant_range(stages: vk::ShaderStageFlags) -> vk::PushConstantRange {
        vk::PushConstantRange::default()
            .stage_flags(stages)
            .offset(PUSH_GLOBAL_DELTA_OFFSET)
            .size(PUSH_GLOBAL_DELTA_SIZE)
    }

    /// # Safety
    /// `command` must be recording on this device and `layout` must include
    /// push_constant_range(stages). Push again before every global draw or dispatch.
    pub unsafe fn push_delta(
        &self,
        command: vk::CommandBuffer,
        layout: vk::PipelineLayout,
        stages: vk::ShaderStageFlags,
    ) {
        self.device.cmd_push_constants(
            command,
            layout,
            stages,
            PUSH_GLOBAL_DELTA_OFFSET,
            &self.addresses.delta().to_ne_bytes(),
        );
    }
}

impl Drop for GlobalMemory {
    fn drop(&mut self) {
        // SAFETY: the owning device outlives this object and all submitted users
        // finish before either the buffer or its memory is freed.
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_buffer(self.buffer, None);
            self.device.free_memory(self.memory, None);
        }
    }
}
