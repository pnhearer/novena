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

/// Program data starts with a magic word that selects the layout (observed
/// on a real program, docs/provenance): 0x12345678 is a graphics program
/// with the 80-byte shader header at +0x30 and code at +0x80; 0x12345679 is
/// a compute program with no header and code at +0x100. The translator gets
/// the header followed by the code; a compute program gets an all-zero
/// header, which parses as the compute shader type.
const GRAPHICS_MAGIC: u32 = 0x1234_5678;
const COMPUTE_MAGIC: u32 = 0x1234_5679;
const COMPUTE_CODE_OFFSET: usize = 0x100;
const SHADER_HEADER_SIZE: usize = SHADER_CODE_OFFSET - SHADER_PREFIX;

/// Header followed by code, from program memory read from its start.
fn layout_bytes(memory: &[u8]) -> Result<Vec<u8>, &'static str> {
    if memory.len() <= SHADER_CODE_OFFSET {
        return Err("no code after header");
    }
    let magic = u32::from_le_bytes(memory[..4].try_into().unwrap());
    let (header, code_offset) = match magic {
        COMPUTE_MAGIC => ([0u8; SHADER_HEADER_SIZE], COMPUTE_CODE_OFFSET),
        GRAPHICS_MAGIC => (
            memory[SHADER_PREFIX..SHADER_CODE_OFFSET]
                .try_into()
                .unwrap(),
            SHADER_CODE_OFFSET,
        ),
        _ => return Err("unknown shader layout magic"),
    };
    let code = memory.get(code_offset..).unwrap_or(&[]);
    let length = (0..code.len().saturating_sub(63))
        .step_by(4)
        .find(|&start| code[start..start + 64].iter().all(|&byte| byte == 0))
        .unwrap_or(code.len());
    if length == 0 {
        return Err("no code after header");
    }
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&code[..length]);
    Ok(bytes)
}

fn shader_bytes(instance: &Instance, address: u64) -> Result<Vec<u8>, &'static str> {
    let resolution = instance
        .objects
        .resolve_gpu_address(address)
        .map_err(|_| "shader bytes unavailable")?;
    let available =
        usize::try_from(resolution.remaining).map_err(|_| "shader bytes unavailable")?;
    if available <= SHADER_CODE_OFFSET {
        return Err("shader bytes unavailable");
    }
    let limit = available.min(MAX_SHADER_BYTES);
    let mut prefix = [0u8; SHADER_CODE_OFFSET];
    if !instance.read_memory(resolution.program_address, &mut prefix) {
        return Err("shader bytes unavailable");
    }
    let magic = u32::from_le_bytes(prefix[..4].try_into().unwrap());
    let (header, code_offset) = match magic {
        COMPUTE_MAGIC => ([0u8; SHADER_HEADER_SIZE], COMPUTE_CODE_OFFSET),
        GRAPHICS_MAGIC => (
            prefix[SHADER_PREFIX..SHADER_CODE_OFFSET]
                .try_into()
                .unwrap(),
            SHADER_CODE_OFFSET,
        ),
        _ => return Err("unknown shader layout magic"),
    };
    if limit <= code_offset {
        return Err("no code after header");
    }
    let mut code = vec![0; limit - code_offset];
    let mut length = code.len();
    let mut chunk = [0u8; 256];
    for offset in (0..code.len()).step_by(chunk.len()) {
        let read_length = chunk.len().min(code.len() - offset);
        let at = resolution
            .program_address
            .checked_add((code_offset + offset) as u64)
            .ok_or("shader bytes unavailable")?;
        if !instance.read_memory(at, &mut chunk[..read_length]) {
            return Err("shader bytes unavailable");
        }
        code[offset..offset + read_length].copy_from_slice(&chunk[..read_length]);
        if let Some(start) = (0..read_length.saturating_sub(63))
            .step_by(4)
            .find(|&start| chunk[start..start + 64].iter().all(|&byte| byte == 0))
        {
            length = offset + start;
            break;
        }
    }
    if length == 0 {
        return Err("no code after header");
    }
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(&code[..length]);
    Ok(bytes)
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
        let Ok(program_bytes) = layout_bytes(&bytes) else {
            instance.fail_pending_shader(program, entry.record_index);
            continue;
        };
        let Some(translator) = instance.shader_translator() else {
            continue;
        };
        match translator.translate(SHADER_STAGE_UNKNOWN, &program_bytes) {
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
    fn program_layout_follows_the_magic_word() {
        let mut graphics = vec![0u8; 0x200];
        graphics[..4].copy_from_slice(&GRAPHICS_MAGIC.to_le_bytes());
        graphics[0x30] = 0xaa;
        graphics[0x80..0x88].copy_from_slice(&[1; 8]);
        let bytes = layout_bytes(&graphics).unwrap();
        assert_eq!((bytes.len(), bytes[0], bytes[0x50]), (0x50 + 8, 0xaa, 1));

        let mut compute = vec![0u8; 0x200];
        compute[..4].copy_from_slice(&COMPUTE_MAGIC.to_le_bytes());
        compute[0x100..0x108].copy_from_slice(&[2; 8]);
        let bytes = layout_bytes(&compute).unwrap();
        assert!(bytes[..0x50].iter().all(|&b| b == 0));
        assert_eq!(&bytes[0x50..], &[2; 8]);

        assert!(layout_bytes(&[0u8; 0x200]).is_err());
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
        // Graphics program layout: magic, header at +0x30, code at +0x80.
        memory.bytes[0..4].copy_from_slice(&GRAPHICS_MAGIC.to_le_bytes());
        memory.bytes[code..code + 4].copy_from_slice(&1u32.to_le_bytes());
        memory.bytes[code + 4..code + 8].copy_from_slice(&2u32.to_le_bytes());
        let host = Host {
            user: (&mut *memory as *mut Memory).cast(),
            read_memory: Some(read_memory),
            write_memory: None,
            present: None,
            render_scale: 1.0,
            wait_vblank: None,
            vulkan: std::ptr::null(),
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
                observed_gpu_address: None,
            },
        );
        let seen = Arc::new(Mutex::new(Vec::new()));
        instance.set_shader_translator(Some(Arc::new(FakeTranslator(seen.clone()))));
        instance.set_shader_translation_enabled(true);
        let dump_directory =
            std::env::temp_dir().join(format!("novena-spirv-dump-test-{}", std::process::id()));
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
            dump_record(instance, record);
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

/// Opt-in observation for working out shader record layouts: when
/// NOVENA_RECORD_DUMP_DIR is set, write each SetShaders record's raw words
/// and, for every word that resolves to program memory, the bytes from
/// 0x80 before it to 0x180 after it. Written once per record address. The
/// output contains program data and belongs in a private directory.
fn dump_record(instance: &Instance, record: &ShaderRecord) {
    let Some(directory) = std::env::var_os("NOVENA_RECORD_DUMP_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    // Programs reuse record memory, so key by the record's contents.
    let key = record
        .raw_words
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, w| {
            (h ^ w).wrapping_mul(0x0100_0000_01b3)
        });
    let path = directory.join(format!("{key:016x}.txt"));
    if path.exists() {
        return;
    }
    let mut text = String::new();
    for (index, word) in record.raw_words.iter().enumerate() {
        text.push_str(&format!("word +0x{:02x}: {word:#018x}\n", index * 8));
    }
    for (index, word) in record.raw_words.iter().enumerate() {
        let Ok(resolution) = instance.objects.resolve_gpu_address(*word) else {
            continue;
        };
        let start = resolution.program_address.saturating_sub(0x80);
        let mut bytes = vec![0u8; 0x200];
        if !instance.read_memory(start, &mut bytes) {
            continue;
        }
        text.push_str(&format!(
            "window for word +0x{:02x} (program address {:#x}, from -0x80):\n",
            index * 8,
            resolution.program_address
        ));
        for (row, chunk) in bytes.chunks(16).enumerate() {
            let hex = chunk
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            text.push_str(&format!("  {:+5}: {hex}\n", row as i64 * 16 - 0x80));
        }
    }
    let _ = fs::create_dir_all(&directory).and_then(|()| fs::write(path, text));
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
            #[cfg(feature = "vulkan")]
            if let Some(backend) = instance
                .gpu
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_mut()
            {
                backend.release_texture(registers.x[0]);
            }
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
        // Pool aliases use the same flat address plus the texture offset.
        "nvnTextureGetTextureAddress" => |instance, _, registers| {
            let Some(Object::Texture { description, .. }) = instance.objects.get(registers.x[0])
            else {
                return Status::BadArgument;
            };
            let (base, size) = match instance.objects.get(description.pool) {
                Some(Object::MemoryPool {
                    storage,
                    gpu_address,
                    observed_gpu_address,
                    size,
                    ..
                }) => (
                    gpu_address.or(observed_gpu_address).unwrap_or(storage),
                    size,
                ),
                _ => return Status::BadArgument,
            };
            if description.pool_offset >= size {
                return Status::BadArgument;
            }
            let Some(address) = base.checked_add(description.pool_offset) else {
                return Status::BadArgument;
            };
            registers.x[0] = address;
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
