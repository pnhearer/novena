//! Textures, texture views, samplers, the pools that register them, and
//! programs. Signatures: docs/signatures/0001-setup.md, 0003-objects.md,
//! 0004-pointers.md.

use super::{
    objects::{Object, SamplerDescription, TextureDescription},
    read_f32x4, succeed, Handler,
};
use crate::instance::{Instance, Registers, Status};

fn texture_builder_update(
    instance: &Instance,
    registers: &Registers,
    change: impl FnOnce(&mut TextureDescription),
) -> Status {
    instance.objects.update(registers.x[0], |object| {
        if let Object::TextureBuilder(description) = object {
            change(description);
        }
    });
    Status::Ok
}

fn sampler_builder_update(
    instance: &Instance,
    registers: &Registers,
    change: impl FnOnce(&mut SamplerDescription),
) -> Status {
    instance.objects.update(registers.x[0], |object| {
        if let Object::SamplerBuilder(description) = object {
            change(description);
        }
    });
    Status::Ok
}

fn texture_field(
    instance: &Instance,
    address: u64,
    pick: impl FnOnce(&TextureDescription) -> u64,
) -> Option<u64> {
    match instance.objects.get(address) {
        Some(Object::Texture { description, .. }) => Some(pick(&description)),
        _ => None,
    }
}

/// How much pool storage a texture needs, by novena's own reckoning.
///
/// The real implementation's answer depends on the format and its tiling,
/// neither of which is known. Four bytes per texel per level, with each
/// level at least one texel, is enough for every format a Vulkan
/// implementation would store uncompressed at 32 bits, and never smaller
/// than what the program will later read or write through the pool.
fn storage_size(description: &TextureDescription) -> u64 {
    let (mut width, mut height) = (description.width.max(1), description.height.max(1));
    let depth = description.depth.max(1);
    let mut total = 0;
    for _ in 0..description.levels.max(1) {
        total += width * height * depth * 4;
        width = (width / 2).max(1);
        height = (height / 2).max(1);
    }
    total.next_multiple_of(STORAGE_ALIGNMENT)
}

/// The largest alignment the real implementation was seen to ask for.
const STORAGE_ALIGNMENT: u64 = 0x10000;

pub fn handler(name: &str) -> Option<Handler> {
    Some(match name {
        "nvnTextureBuilderSetDefaults" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::TextureBuilder(TextureDescription::default()),
            );
            Status::Ok
        },
        "nvnTextureBuilderSetDevice" => |instance, _, registers| {
            let value = registers.x[1];
            texture_builder_update(instance, registers, |t| t.device = value)
        },
        "nvnTextureBuilderSetFlags" => |instance, _, registers| {
            let value = registers.x[1];
            texture_builder_update(instance, registers, |t| t.flags = value)
        },
        "nvnTextureBuilderSetTarget" => |instance, _, registers| {
            let value = registers.x[1];
            texture_builder_update(instance, registers, |t| t.target = value)
        },
        "nvnTextureBuilderSetFormat" => |instance, _, registers| {
            let value = registers.x[1];
            texture_builder_update(instance, registers, |t| t.format = value)
        },
        "nvnTextureBuilderSetLevels" => |instance, _, registers| {
            let value = registers.x[1];
            texture_builder_update(instance, registers, |t| t.levels = value)
        },
        "nvnTextureBuilderSetSize1D" => |instance, _, registers| {
            let value = registers.x[1];
            texture_builder_update(instance, registers, |t| {
                t.width = value;
                t.height = 1;
                t.depth = 1;
            })
        },
        "nvnTextureBuilderSetSize2D" => |instance, _, registers| {
            let (w, h) = (registers.x[1], registers.x[2]);
            texture_builder_update(instance, registers, |t| {
                t.width = w;
                t.height = h;
                t.depth = 1;
            })
        },
        "nvnTextureBuilderSetSize3D" => |instance, _, registers| {
            let (w, h, d) = (registers.x[1], registers.x[2], registers.x[3]);
            texture_builder_update(instance, registers, |t| {
                t.width = w;
                t.height = h;
                t.depth = d;
            })
        },
        "nvnTextureBuilderSetStride" => |instance, _, registers| {
            let value = registers.x[1];
            texture_builder_update(instance, registers, |t| t.stride = value)
        },
        "nvnTextureBuilderSetSwizzle" => |instance, _, registers| {
            let value = [
                registers.x[1],
                registers.x[2],
                registers.x[3],
                registers.x[4],
            ];
            texture_builder_update(instance, registers, |t| t.swizzle = value)
        },
        "nvnTextureBuilderSetDepthStencilMode" => |instance, _, registers| {
            let value = registers.x[1];
            texture_builder_update(instance, registers, |t| t.depth_stencil_mode = value)
        },
        "nvnTextureBuilderSetStorage" => |instance, _, registers| {
            let (pool, offset) = (registers.x[1], registers.x[2]);
            texture_builder_update(instance, registers, |t| {
                t.pool = pool;
                t.pool_offset = offset;
            })
        },
        "nvnTextureBuilderGetStorageSize" => {
            |instance, _, registers| match instance.objects.get(registers.x[0]) {
                Some(Object::TextureBuilder(description)) => {
                    registers.x[0] = storage_size(&description);
                    Status::Ok
                }
                _ => Status::BadArgument,
            }
        }
        "nvnTextureBuilderGetStorageAlignment" => |_instance, _, registers| {
            registers.x[0] = STORAGE_ALIGNMENT;
            Status::Ok
        },
        "nvnTextureInitialize" => |instance, _, registers| {
            let Some(Object::TextureBuilder(description)) = instance.objects.get(registers.x[1])
            else {
                return Status::BadArgument;
            };
            instance.objects.put(
                registers.x[0],
                Object::Texture {
                    description,
                    // The image is shared so object-table clones stay cheap.
                    image: std::sync::Arc::new(std::sync::Mutex::new(None)),
                },
            );
            succeed(registers)
        },
        "nvnTextureFinalize" => |instance, _, registers| {
            instance.objects.remove(registers.x[0]);
            Status::Ok
        },
        "nvnTextureGetFlags" => {
            |instance, _, registers| texture_answer(instance, registers, |t| t.flags)
        }
        "nvnTextureGetTarget" => {
            |instance, _, registers| texture_answer(instance, registers, |t| t.target)
        }
        "nvnTextureGetLevels" => {
            |instance, _, registers| texture_answer(instance, registers, |t| t.levels)
        }
        // Observed to answer 0 for every texture the program asked about.
        "nvnTextureGetSamples" => {
            |instance, _, registers| texture_answer(instance, registers, |_| 0)
        }
        // The texture's bytes live in its pool's storage at the offset the
        // program chose; see the note in memory.rs on graphics addresses.
        "nvnTextureGetTextureAddress" => |instance, _, registers| {
            let Some(Object::Texture { description, .. }) = instance.objects.get(registers.x[0])
            else {
                return Status::BadArgument;
            };
            let storage = match instance.objects.get(description.pool) {
                Some(Object::MemoryPool { storage, .. }) => storage,
                _ => 0,
            };
            registers.x[0] = storage + description.pool_offset;
            Status::Ok
        },
        // A view's offset into the texture's storage. Levels are laid out
        // in order by novena's own storage_size rule.
        "nvnTextureGetViewOffset" => |instance, _, registers| {
            let Some(Object::Texture { description, .. }) = instance.objects.get(registers.x[0])
            else {
                return Status::BadArgument;
            };
            let base_level = match instance.objects.get(registers.x[1]) {
                Some(Object::TextureView { base_level, .. }) => base_level,
                _ => 0,
            };
            let mut offset = 0;
            let (mut width, mut height) = (description.width.max(1), description.height.max(1));
            for _ in 0..base_level {
                offset += width * height * description.depth.max(1) * 4;
                width = (width / 2).max(1);
                height = (height / 2).max(1);
            }
            registers.x[0] = offset;
            Status::Ok
        },
        // The program asks how much memory a depth texture's culling data
        // needs and later saves and restores it. Novena keeps none, so the
        // smallest observed answer is enough to keep the program's layout.
        "nvnTextureGetZCullStorageSize" => |_instance, _, registers| {
            registers.x[0] = 0x100;
            Status::Ok
        },
        "nvnTextureViewSetDefaults" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::TextureView {
                    base_level: 0,
                    levels: 0,
                    base_layer: 0,
                    layers: 0,
                },
            );
            Status::Ok
        },
        "nvnTextureViewSetLevels" => |instance, _, registers| {
            let (base, count) = (registers.x[1] as u32, registers.x[2] as u32);
            instance.objects.update(registers.x[0], |object| {
                if let Object::TextureView {
                    base_level, levels, ..
                } = object
                {
                    *base_level = base;
                    *levels = count;
                }
            });
            Status::Ok
        },
        "nvnTextureViewSetLayers" => |instance, _, registers| {
            let (base, count) = (registers.x[1] as u32, registers.x[2] as u32);
            instance.objects.update(registers.x[0], |object| {
                if let Object::TextureView {
                    base_layer, layers, ..
                } = object
                {
                    *base_layer = base;
                    *layers = count;
                }
            });
            Status::Ok
        },
        "nvnTexturePoolInitialize" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::TexturePool {
                    memory: registers.x[1],
                    offset: registers.x[2],
                    count: registers.x[3],
                    registered: Default::default(),
                },
            );
            succeed(registers)
        },
        "nvnSamplerPoolInitialize" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::SamplerPool {
                    memory: registers.x[1],
                    offset: registers.x[2],
                    count: registers.x[3],
                    registered: Default::default(),
                },
            );
            succeed(registers)
        },
        "nvnTexturePoolRegisterTexture" | "nvnTexturePoolRegisterImage" => {
            |instance, _, registers| {
                let (id, texture) = (registers.x[1] as u32, registers.x[2]);
                instance.objects.update(registers.x[0], |object| {
                    if let Object::TexturePool { registered, .. } = object {
                        registered.insert(id, texture);
                    }
                });
                Status::Ok
            }
        }
        "nvnSamplerPoolRegisterSampler" => |instance, _, registers| {
            let (id, sampler) = (registers.x[1] as u32, registers.x[2]);
            instance.objects.update(registers.x[0], |object| {
                if let Object::SamplerPool { registered, .. } = object {
                    registered.insert(id, sampler);
                }
            });
            Status::Ok
        },
        "nvnSamplerBuilderSetDefaults" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::SamplerBuilder(SamplerDescription::default()),
            );
            Status::Ok
        },
        "nvnSamplerBuilderSetDevice" => |instance, _, registers| {
            let value = registers.x[1];
            sampler_builder_update(instance, registers, |s| s.device = value)
        },
        "nvnSamplerBuilderSetMinMagFilter" => |instance, _, registers| {
            let (min, mag) = (registers.x[1], registers.x[2]);
            sampler_builder_update(instance, registers, |s| {
                s.min_filter = min;
                s.mag_filter = mag;
            })
        },
        "nvnSamplerBuilderSetWrapMode" => |instance, _, registers| {
            let wrap = [registers.x[1], registers.x[2], registers.x[3]];
            sampler_builder_update(instance, registers, |s| s.wrap = wrap)
        },
        "nvnSamplerBuilderSetMaxAnisotropy" => |instance, _, registers| {
            let value = f32::from_bits(registers.d[0] as u32);
            sampler_builder_update(instance, registers, |s| s.max_anisotropy = value)
        },
        "nvnSamplerBuilderSetCompare" => |instance, _, registers| {
            let (mode, func) = (registers.x[1], registers.x[2]);
            sampler_builder_update(instance, registers, |s| {
                s.compare_mode = mode;
                s.compare_func = func;
            })
        },
        "nvnSamplerBuilderSetBorderColor" => |instance, _, registers| {
            let Some(color) = read_f32x4(instance, registers.x[1]) else {
                return Status::BadArgument;
            };
            sampler_builder_update(instance, registers, |s| s.border_color = color)
        },
        "nvnSamplerBuilderSetLodBias" => |instance, _, registers| {
            let value = f32::from_bits(registers.d[0] as u32);
            sampler_builder_update(instance, registers, |s| s.lod_bias = value)
        },
        "nvnSamplerBuilderSetLodClamp" => |instance, _, registers| {
            let value = [
                f32::from_bits(registers.d[0] as u32),
                f32::from_bits(registers.d[1] as u32),
            ];
            sampler_builder_update(instance, registers, |s| s.lod_clamp = value)
        },
        "nvnSamplerInitialize" => |instance, _, registers| {
            let Some(Object::SamplerBuilder(description)) = instance.objects.get(registers.x[1])
            else {
                return Status::BadArgument;
            };
            instance
                .objects
                .put(registers.x[0], Object::Sampler(description));
            succeed(registers)
        },
        "nvnProgramInitialize" => |instance, _, registers| {
            instance.objects.put(
                registers.x[0],
                Object::Program {
                    device: registers.x[1],
                    shader_records: Vec::new(),
                },
            );
            succeed(registers)
        },
        // The records' contents are the open shader question. Their
        // addresses are kept so a later stage can read them.
        "nvnProgramSetShaders" => |instance, _, registers| {
            let count = registers.x[1].min(8);
            let records: Vec<u64> = (0..count)
                .map(|index| registers.x[2] + index * 0x40)
                .collect();
            instance.objects.update(registers.x[0], |object| {
                if let Object::Program { shader_records, .. } = object {
                    *shader_records = records;
                }
            });
            succeed(registers)
        },
        _ => return None,
    })
}

fn texture_answer(
    instance: &Instance,
    registers: &mut Registers,
    pick: impl FnOnce(&TextureDescription) -> u64,
) -> Status {
    match texture_field(instance, registers.x[0], pick) {
        Some(value) => {
            registers.x[0] = value;
            Status::Ok
        }
        None => Status::BadArgument,
    }
}
