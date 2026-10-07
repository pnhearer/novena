//! novena's record of the program's objects.
//!
//! The program allocates every object itself and passes its address. novena
//! keeps what it knows about each object here, keyed by that address, and
//! never writes into the program's object memory. That way the size and
//! layout of the objects need not be known. It assumes the program only
//! touches objects through the API, which has not been contradicted by
//! observation but has not been confirmed either (see docs/design.md).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Clone, PartialEq)]
pub enum RecordedCommand {
    SetRenderTargets {
        colors: Vec<u64>,
        depth: u64,
    },
    ClearColor {
        index: u32,
        color: [f32; 4],
        mask: u32,
    },
    ClearDepthStencil {
        depth: f32,
        depth_write: u32,
        stencil: u32,
        stencil_mask: u32,
    },
    CopyBufferToTexture {
        buffer: u64,
        texture: u64,
    },
    CopyTextureToTexture {
        source: u64,
        destination: u64,
    },
    SetViewport([u64; 5]),
    SetScissor([u64; 5]),
    SetDepthRange([u64; 3]),
    Raw {
        function: u32,
        registers: [u64; 8],
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextureImage {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderRecord {
    pub record_address: u64,
    pub gpu_addresses: [u64; 2],
    pub raw_words: [u64; 8],
}

/// What novena knows about one object. Field meanings follow the signature
/// tables; unknown enumerations are kept as the integers the program passed.
#[derive(Debug, Clone, Default)]
pub enum Object {
    #[default]
    Unknown,
    DeviceBuilder {
        flags: u64,
    },
    Device {
        flags: u64,
        depth_mode: u64,
        window_origin_mode: u64,
    },
    QueueBuilder {
        device: u64,
        control_memory_size: u64,
        command_flush_threshold: u64,
    },
    Queue {
        device: u64,
    },
    WindowBuilder {
        device: u64,
        native_window: u64,
        textures: Vec<u64>,
    },
    Window {
        device: u64,
        textures: Vec<u64>,
        present_interval: u32,
        /// Index of the texture the next acquire hands out.
        next_texture: u32,
    },
    Sync {
        device: u64,
    },
    EventBuilder {
        pool: u64,
        offset: u64,
    },
    Event {
        value: u32,
        /// Program address of the event's storage word, which the program
        /// may read directly instead of calling EventGetValue.
        storage: u64,
    },
    MemoryPoolBuilder {
        device: u64,
        flags: u64,
        storage: u64,
        size: u64,
    },
    MemoryPool {
        device: u64,
        flags: u64,
        storage: u64,
        size: u64,
    },
    TextureBuilder(TextureDescription),
    Texture {
        description: TextureDescription,
        image: Arc<Mutex<Option<TextureImage>>>,
    },
    TextureView {
        base_level: u32,
        levels: u32,
        base_layer: u32,
        layers: u32,
    },
    TexturePool {
        memory: u64,
        offset: u64,
        count: u64,
        registered: HashMap<u32, u64>,
    },
    SamplerPool {
        memory: u64,
        offset: u64,
        count: u64,
        registered: HashMap<u32, u64>,
    },
    SamplerBuilder(SamplerDescription),
    Sampler(SamplerDescription),
    Program {
        device: u64,
        shader_records: Vec<ShaderRecord>,
    },
    /// A state object. Its fields are not interpreted yet; the program's
    /// calls are kept as (function, arguments) so they are not lost.
    State {
        kind: &'static str,
        settings: Vec<(&'static str, [u64; 6])>,
    },
    CommandBuffer {
        device: u64,
        command_memory: Vec<(u64, u64, u64)>,
        control_memory: Vec<(u64, u64)>,
        recording: bool,
        recordings: u64,
        commands: Vec<RecordedCommand>,
        recording_handles: HashMap<u64, Vec<RecordedCommand>>,
    },
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Object::Texture { description: a, .. }, Object::Texture { description: b, .. }) => {
                a == b
            }
            (a, b) => format!("{a:?}") == format!("{b:?}"),
        }
    }
}

/// What the program told a texture builder, carried over to the texture.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextureDescription {
    pub device: u64,
    pub flags: u64,
    pub target: u64,
    pub format: u64,
    pub levels: u64,
    pub width: u64,
    pub height: u64,
    pub depth: u64,
    pub stride: u64,
    pub swizzle: [u64; 4],
    pub depth_stencil_mode: u64,
    pub pool: u64,
    pub pool_offset: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SamplerDescription {
    pub device: u64,
    pub min_filter: u64,
    pub mag_filter: u64,
    pub wrap: [u64; 3],
    pub max_anisotropy: f32,
    pub compare_mode: u64,
    pub compare_func: u64,
    pub border_color: [f32; 4],
    pub lod_bias: f32,
    pub lod_clamp: [f32; 2],
}

#[derive(Default)]
pub struct Objects {
    map: Mutex<HashMap<u64, Object>>,
}

impl Objects {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<u64, Object>> {
        self.map
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Replace whatever was at `address` with `object`. A builder's
    /// SetDefaults call and every Initialize call start an object afresh.
    pub fn put(&self, address: u64, object: Object) {
        self.lock().insert(address, object);
    }

    pub fn get(&self, address: u64) -> Option<Object> {
        self.lock().get(&address).cloned()
    }

    pub fn remove(&self, address: u64) -> Option<Object> {
        self.lock().remove(&address)
    }

    /// Change the object at `address` in place. Returns false when there is
    /// no object there, which means the program used an object it never set
    /// up through the API, or one novena did not record.
    pub fn update(&self, address: u64, change: impl FnOnce(&mut Object)) -> bool {
        match self.lock().get_mut(&address) {
            Some(object) => {
                change(object);
                true
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Every texture some window presents.
    pub fn window_textures(&self) -> std::collections::HashSet<u64> {
        self.lock()
            .values()
            .filter_map(|object| match object {
                Object::Window { textures, .. } => Some(textures.iter().copied()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Take the commands of a submitted recording. A recording handle
    /// carries its command buffer's address in its low 48 bits (see
    /// EndRecording), so the lookup is direct. A recording is executed once
    /// and then dropped, so finished recordings do not pile up.
    pub fn recording(&self, handle: u64) -> Option<Vec<RecordedCommand>> {
        let owner = handle & ((1 << 48) - 1);
        match self.lock().get_mut(&owner) {
            Some(Object::CommandBuffer {
                recording_handles, ..
            }) => recording_handles.remove(&handle),
            _ => None,
        }
    }
}
