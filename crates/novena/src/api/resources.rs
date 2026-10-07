//! Textures, texture views, samplers, the pools that register them, and
//! programs. Signatures: docs/signatures/0001-setup.md, 0003-objects.md,
//! 0004-pointers.md.

use super::{
    objects::{Object, SamplerDescription, ShaderRecord, ShaderTranslation, TextureDescription},
    read_f32x4, succeed, Handler,
};
use crate::instance::{Instance, PendingShader, Registers, Status, SHADER_STAGE_UNKNOWN};
use std::fs;

const MAX_SHADER_BYTES: usize = 64 * 1024;
const SHADER_PREFIX: usize = 0x30;
const SHADER_CODE_OFFSET: usize = 0x80;

fn shader_bytes(instance: &Instance, address: u64) -> Result<Vec<u8>, &'static str> {
    let resolution = instance
        .objects
        .resolve_gpu_address(address)
        .map_err(|_| "shader bytes unavailable")?;
    let available =
        usize::try_from(resolution.remaining).map_err(|_| "shader bytes unavailable")?;
    if available <= SHADER_PREFIX {
        return Err("shader bytes unavailable");
    }
    let limit = available.min(MAX_SHADER_BYTES);
    let mut bytes = vec![0; limit];
    let mut length = limit;
    let mut chunk = [0u8; 256];
    let scan_start = SHADER_CODE_OFFSET.min(limit);
    for offset in (scan_start..limit).step_by(chunk.len()) {
        let read_length = chunk.len().min(limit - offset);
        let address = resolution
            .program_address
            .checked_add(offset as u64)
            .ok_or("shader bytes unavailable")?;
        if !instance.read_memory(address, &mut chunk[..read_length]) {
            return Err("shader bytes unavailable");
        }
        bytes[offset..offset + read_length].copy_from_slice(&chunk[..read_length]);
        for start in (0..read_length.saturating_sub(63)).step_by(4) {
            if chunk[start..start + 64].iter().all(|&byte| byte == 0) {
                length = offset + start;
                break;
            }
        }
        if length != limit {
            break;
        }
    }
    if length <= SHADER_CODE_OFFSET {
        return Err("no code after header");
    }
    Ok(bytes[SHADER_PREFIX..length].to_vec())
}

pub(crate) fn retry_pending(instance: &Instance, program: u64, point: &'static str) {
    let entries = instance.pending_shaders(program);
    for entry in entries {
        if entry.failures >= 64 {
            continue;
        }
        let mut bytes = vec![0; entry.limit];
        if !instance.read_memory(entry.address, &mut bytes) {
            instance.fail_pending_shader(program, entry.record_index);
            continue;
        }
        let end = bytes.len();
        let mut code_end = end;
        for start in (SHADER_CODE_OFFSET..end.saturating_sub(63)).step_by(4) {
            if bytes[start..start + 64].iter().all(|&byte| byte == 0) {
                code_end = start;
                break;
            }
        }
        if code_end <= SHADER_CODE_OFFSET {
            instance.fail_pending_shader(program, entry.record_index);
            continue;
        }
        let Some(translator) = instance.shader_translator() else {
            continue;
        };
        match translator.translate(SHADER_STAGE_UNKNOWN, &bytes[SHADER_PREFIX..code_end]) {
            Ok(words) => {
                instance.record_successful_shader_translation(&words);
                instance.record_late_read(point);
                instance.replace_pending_shader(program, entry.record_index);
                instance.objects.update(program, |object| {
                    if let Object::Program {
                        shader_translations,
                        ..
                    } = object
                    {
                        if let Some(slot) = shader_translations.get_mut(entry.record_index) {
                            *slot = ShaderTranslation::Spirv(words.clone());
                        }
                    }
                });
            }
            Err(error) => {
                instance.record_shader_translation_error(error);
                instance.fail_pending_shader(program, entry.record_index);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::api::{Object, ShaderTranslation};
    use crate::{Host, Registers, ShaderStage, ShaderTranslator, SHADER_STAGE_UNKNOWN};
    use std::ffi::c_void;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    struct Memory {
        base: u64,
        bytes: Vec<u8>,
    }

    unsafe extern "C" fn read_memory(
        user: *mut c_void,
        address: u64,
        out: *mut u8,
        size: u64,
    ) -> i32 {
        let memory = unsafe { &*(user.cast::<Memory>()) };
        let Some(start) = address.checked_sub(memory.base).map(|value| value as usize) else {
            return 1;
        };
        let end = start.saturating_add(size as usize);
        if end > memory.bytes.len() {
            return 1;
        }
        unsafe { std::slice::from_raw_parts_mut(out, size as usize) }
            .copy_from_slice(&memory.bytes[start..end]);
        0
    }

    type SeenTranslations = Arc<Mutex<Vec<(ShaderStage, Vec<u8>)>>>;

    struct FakeTranslator(SeenTranslations);

    impl ShaderTranslator for FakeTranslator {
        fn translate(&self, stage: ShaderStage, code: &[u8]) -> Result<Vec<u32>, String> {
            self.0.lock().unwrap().push((stage, code.to_vec()));
            Ok(vec![0x0723_0203, code.len() as u32])
        }
    }

    #[test]
    fn enabled_translation_uses_resolved_bytes_and_retains_spirv() {
        let base = 0x1000;
        let mut memory = Box::new(Memory {
            base,
            bytes: vec![0; 0x4000],
        });
        let record = 0x2000usize - base as usize;
        let code = 0x1080usize - base as usize;
        memory.bytes[record..record + 8].copy_from_slice(&0x1000u64.to_le_bytes());
        memory.bytes[code..code + 4].copy_from_slice(&1u32.to_le_bytes());
        memory.bytes[code + 4..code + 8].copy_from_slice(&2u32.to_le_bytes());
        let host = Host {
            user: (&mut *memory as *mut Memory).cast(),
            read_memory: Some(read_memory),
            write_memory: None,
            present: None,
            render_scale: 1.0,
            wait_vblank: None,
        };
        let instance = unsafe { Instance::with_host(host) };
        instance.objects.put(
            7,
            Object::MemoryPool {
                device: 0,
                flags: 0,
                storage: base,
                size: 0x4000,
                gpu_address: None,
            },
        );
        let seen = Arc::new(Mutex::new(Vec::new()));
        instance.set_shader_translator(Some(Arc::new(FakeTranslator(seen.clone()))));
        instance.set_shader_translation_enabled(true);
        let dump_directory = PathBuf::from(format!(
            "/tmp/novena-spirv-dump-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dump_directory);
        instance.set_shader_dump_directory(Some(&dump_directory));
        let mut init = Registers {
            x: [9, 0, 0, 0, 0, 0, 0, 0],
            ..Registers::default()
        };
        assert_eq!(
            instance.call(
                crate::functions::lookup("nvnProgramInitialize").unwrap(),
                &mut init
            ),
            Status::Ok
        );
        let mut setup = Registers {
            x: [9, 1, 0x2000, 0, 0, 0, 0, 0],
            ..Registers::default()
        };
        assert_eq!(
            instance.call(
                crate::functions::lookup("nvnProgramSetShaders").unwrap(),
                &mut setup
            ),
            Status::Ok
        );
        assert_eq!(seen.lock().unwrap()[0].0, SHADER_STAGE_UNKNOWN);
        assert_eq!(seen.lock().unwrap()[0].1.len(), 0x50 + 8);
        assert_eq!(
            u32::from_le_bytes(seen.lock().unwrap()[0].1[0x50..0x54].try_into().unwrap()),
            1
        );
        assert_eq!(
            instance.objects.get(9),
            Some(Object::Program {
                device: 0,
                shader_records: vec![crate::api::ShaderRecord {
                    record_address: 0x2000,
                    gpu_addresses: [0x1000, 0],
                    raw_words: [0x1000, 0, 0, 0, 0, 0, 0, 0]
                }],
                shader_translations: vec![ShaderTranslation::Spirv(vec![0x0723_0203, 0x50 + 8])]
            })
        );
        assert_eq!(
            fs::read(dump_directory.join("0-7dff408e4ca88672.spv")).unwrap(),
            [0x03, 0x02, 0x23, 0x07, 0x50 + 8, 0, 0, 0]
        );
        fs::remove_dir_all(dump_directory).unwrap();
    }
}

fn translate_shaders(
    instance: &Instance,
    program: u64,
    records: &[ShaderRecord],
) -> Vec<super::objects::ShaderTranslation> {
    if !instance.shader_translation_enabled() {
        return Vec::new();
    }
    let Some(translator) = instance.shader_translator() else {
        return Vec::new();
    };
    records
        .iter()
        .enumerate()
        .map(|(record_index, record)| {
            let code = match shader_bytes(instance, record.gpu_addresses[0]) {
                Ok(code) => code,
                Err(error) => {
                    if error == "no code after header" {
                        if let Ok(resolution) = instance
                            .objects
                            .resolve_gpu_address(record.gpu_addresses[0])
                        {
                            let limit = usize::try_from(resolution.remaining)
                                .unwrap_or(0)
                                .min(MAX_SHADER_BYTES);
                            let mut prefix = vec![0; SHADER_CODE_OFFSET.min(limit)];
                            let zero = !prefix.is_empty()
                                && instance.read_memory(resolution.program_address, &mut prefix)
                                && prefix.iter().all(|&byte| byte == 0);
                            instance.record_shader_zero_header(zero);
                            instance.add_pending_shader(
                                program,
                                PendingShader {
                                    record_index,
                                    address: resolution.program_address,
                                    limit,
                                    failures: 0,
                                },
                            );
                        }
                    }
                    if error != "no code after header" {
                        instance.record_shader_translation_error(error.into());
                    }
                    return super::objects::ShaderTranslation::Error(error.into());
                }
            };
            match translator.translate(SHADER_STAGE_UNKNOWN, &code) {
                Ok(words) => {
                    instance.record_successful_shader_translation(&words);
                    dump_translation(instance, &words);
                    super::objects::ShaderTranslation::Spirv(words)
                }
                Err(error) => {
                    dump_translation_error(instance, &error);
                    instance.record_shader_translation_error(error.clone());
                    super::objects::ShaderTranslation::Error(error)
                }
            }
        })
        .collect()
}

fn dump_translation(instance: &Instance, words: &[u32]) {
    let Some(directory) = instance.shader_dump_directory() else {
        return;
    };
    let bytes = words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    let Some((sequence, hash)) = instance.next_shader_dump(&bytes) else {
        return;
    };
    let path = directory.join(format!("{sequence}-{hash:016x}.spv"));
    let _ = fs::create_dir_all(&directory).and_then(|()| fs::write(path, bytes));
}

fn dump_translation_error(instance: &Instance, error: &str) {
    let Some(directory) = instance.shader_dump_directory() else {
        return;
    };
    let message = error.lines().next().unwrap_or("");
    let bytes = format!("{message}\n").into_bytes();
    let Some((sequence, hash)) = instance.next_shader_dump(&bytes) else {
        return;
    };
    let path = directory.join(format!("{sequence}-{hash:016x}.err"));
    let _ = fs::create_dir_all(&directory).and_then(|()| fs::write(path, bytes));
}

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
                    shader_translations: Vec::new(),
                },
            );
            succeed(registers)
        },
        // The record has two GPU-shaped values, but observation does not
        // identify the code value or establish a code size. Keep both and
        // the observed words until that gap is closed.
        "nvnProgramSetShaders" => |instance, _, registers| {
            let count = registers.x[1].min(8);
            let records: Vec<ShaderRecord> = (0..count)
                .filter_map(|index| {
                    let record_address = registers.x[2] + index * 0x40;
                    let mut bytes = [0u8; 0x40];
                    if !instance.read_memory(record_address, &mut bytes) {
                        return None;
                    }
                    let mut raw_words = [0u64; 8];
                    for (index, word) in raw_words.iter_mut().enumerate() {
                        let start = index * 8;
                        *word = u64::from_le_bytes(
                            bytes[start..start + 8].try_into().expect("8 bytes"),
                        );
                    }
                    Some(ShaderRecord {
                        record_address,
                        gpu_addresses: [raw_words[0], raw_words[6]],
                        raw_words,
                    })
                })
                .collect();
            instance.objects.update(registers.x[0], |object| {
                if let Object::Program { shader_records, .. } = object {
                    *shader_records = records;
                }
            });
            let translations = instance
                .objects
                .get(registers.x[0])
                .and_then(|object| match object {
                    Object::Program { shader_records, .. } => {
                        Some(translate_shaders(instance, registers.x[0], &shader_records))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            instance.objects.update(registers.x[0], |object| {
                if let Object::Program {
                    shader_translations,
                    ..
                } = object
                {
                    *shader_translations = translations;
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
