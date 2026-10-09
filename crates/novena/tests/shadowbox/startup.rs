//! Original CPU cache compatibility proof. Evidence: provenance 0031.
use novena::{
    api::{Object, ShaderTranslation},
    startup_cache::{StartupCacheConfig, TranslatedShader, TranslationContext, TranslationStatus},
    Host, Instance, Registers, ShaderTranslator,
};
use shadowbox::{
    interface::{
        GeometryInput, GraphicsState, TessellationDomain, TessellationSpacing, TessellationState,
    },
    CacheContext, TargetGeneration, TranslationOptions,
};
use std::{
    ffi::c_void,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

fn context(context: &TranslationContext) -> Result<CacheContext, String> {
    let generation = match context.generation {
        0 => TargetGeneration::Sm5,
        1 => TargetGeneration::Sm6,
        2 => TargetGeneration::Sm7_5Plus,
        _ => return Err("invalid target generation".into()),
    };
    let geometry_input = context
        .geometry_input
        .map(|input| match input {
            1 => Ok(GeometryInput::Points),
            2 => Ok(GeometryInput::Lines),
            3 => Ok(GeometryInput::LinesAdjacency),
            4 => Ok(GeometryInput::Triangles),
            5 => Ok(GeometryInput::TrianglesAdjacency),
            _ => Err("invalid geometry input".to_string()),
        })
        .transpose()?;
    let tessellation = context
        .tessellation
        .map(|[domain, spacing, clockwise]| {
            if clockwise > 1 {
                return Err("invalid tessellation winding".to_string());
            }
            Ok(TessellationState {
                domain: match domain {
                    0 => TessellationDomain::Isolines,
                    1 => TessellationDomain::Triangles,
                    2 => TessellationDomain::Quads,
                    _ => return Err("invalid tessellation domain".into()),
                },
                spacing: match spacing {
                    0 => TessellationSpacing::Equal,
                    1 => TessellationSpacing::FractionalEven,
                    2 => TessellationSpacing::FractionalOdd,
                    _ => return Err("invalid tessellation spacing".into()),
                },
                clockwise: clockwise != 0,
            })
        })
        .transpose()?;
    Ok(CacheContext {
        generation,
        options: TranslationOptions {
            global_delta_zero: context.global_delta_zero,
        },
        graphics_state: GraphicsState {
            geometry_input,
            tessellation,
        },
    })
}

struct Adapter(AtomicUsize);
impl ShaderTranslator for Adapter {
    fn translate(&self, _: u32, program: &[u8]) -> Result<Vec<u32>, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        shadowbox::translate_header_prefixed(program)
            .map(|o| o.spirv)
            .map_err(|e| e.to_string())
    }
    fn translation_cache_key(
        &self,
        _: u32,
        program: &[u8],
        state: &TranslationContext,
    ) -> Result<Option<String>, String> {
        let context = context(state)?;
        let input = shadowbox::header_prefixed_input(program, context.generation)
            .map_err(|e| e.to_string())?;
        let mut header = [0; 128];
        let length = header.len().min(input.program_record.len());
        header[..length].copy_from_slice(&input.program_record[..length]);
        let words: Vec<_> = header
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let stage = shadowbox::headers::parse(&words)
            .map_err(|e| format!("{e:?}"))?
            .stage;
        if !shadowbox::missing_state_keys(stage, &context.graphics_state).is_empty() {
            return Ok(None);
        }
        shadowbox::translation_cache_key(&input, context)
            .map(Some)
            .map_err(|e| e.to_string())
    }
    fn translate_for_context(
        &self,
        _: u32,
        program: &[u8],
        state: &TranslationContext,
    ) -> Result<TranslatedShader, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let context = context(state)?;
        let input = shadowbox::header_prefixed_input(program, context.generation)
            .map_err(|e| e.to_string())?;
        let output = shadowbox::translate_with_options_and_graphics_state(
            &input,
            context.options,
            &context.graphics_state,
        )
        .map_err(|e| e.to_string())?;
        Ok(TranslatedShader {
            key: String::new(),
            words: output.spirv,
            requires_subgroup_size_32: output.requires_subgroup_size_32,
        })
    }
}

// Original EXIT/NOP bundle, already established by provenance 0023 and 0025.
fn program() -> Vec<u8> {
    let mut program = vec![0; 80];
    for instruction in [
        0,
        0xe300_0000_0007_0000_u64,
        0x50b0_0000_0007_0000,
        0x50b0_0000_0007_0000,
    ] {
        program.extend_from_slice(&instruction.to_le_bytes());
    }
    program
}
struct Memory(Vec<u8>);
unsafe extern "C" fn read(user: *mut c_void, address: u64, out: *mut u8, size: u64) -> i32 {
    let memory = unsafe { &*user.cast::<Memory>() };
    let Some(start) = address
        .checked_sub(0x1000)
        .and_then(|n| usize::try_from(n).ok())
    else {
        return 1;
    };
    let Some(bytes) = memory.0.get(start..start.saturating_add(size as usize)) else {
        return 1;
    };
    unsafe { std::slice::from_raw_parts_mut(out, size as usize) }.copy_from_slice(bytes);
    0
}
fn register(instance: &Instance) -> Arc<TranslatedShader> {
    let mut registers = Registers {
        x: [9, 1, 0x1400, 0, 0, 0, 0, 0],
        ..Default::default()
    };
    assert_eq!(
        instance.call(
            novena::functions::lookup("nvnProgramSetShaders").unwrap(),
            &mut registers
        ),
        novena::Status::Ok
    );
    let Some(Object::Program {
        shader_translations,
        ..
    }) = instance.objects.get(9)
    else {
        panic!("program missing")
    };
    let ShaderTranslation::Cached(request) = &shader_translations[0] else {
        panic!("cached request missing")
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match request.poll() {
            TranslationStatus::Ready(shader) => return shader,
            TranslationStatus::Failed(error) => panic!("{error}"),
            _ => {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
        }
    }
}
fn instance(memory: &mut Memory, directory: PathBuf, translator: Arc<Adapter>) -> Instance {
    let host = Host {
        user: (memory as *mut Memory).cast(),
        read_memory: Some(read),
        ..Default::default()
    };
    let instance = unsafe {
        Instance::with_host_startup_cache(host, StartupCacheConfig::new(directory), translator)
    }
    .unwrap();
    instance.objects.put(
        7,
        Object::MemoryPool {
            device: 0,
            flags: 0,
            storage: 0x1000,
            size: memory.0.len() as u64,
            gpu_address: None,
            observed_gpu_address: None,
        },
    );
    instance.objects.put(
        9,
        Object::Program {
            device: 0,
            shader_records: Vec::new(),
            shader_translations: Vec::new(),
        },
    );
    instance
}

#[test]
fn emitted_cache_is_loaded_at_instance_creation_and_runtime_misses_reopen() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp");
    fs::create_dir_all(&root).unwrap();
    let bytes = program();
    let input = shadowbox::header_prefixed_input(&bytes, TargetGeneration::Sm5).unwrap();
    let output = shadowbox::translate_with_options_and_graphics_state(
        &input,
        Default::default(),
        &Default::default(),
    )
    .unwrap();
    let key =
        shadowbox::emit_translation_cache(&root, &input, Default::default(), &output).unwrap();
    let mut memory = Memory(vec![0; 0x1000]);
    memory.0[..4].copy_from_slice(&0x1234_5679_u32.to_le_bytes());
    memory.0[0x100..0x120].copy_from_slice(&bytes[80..]);
    memory.0[0x400..0x408].copy_from_slice(&0x1000_u64.to_le_bytes());
    let translator = Arc::new(Adapter(AtomicUsize::new(0)));
    let warm_start = Instant::now();
    let warm = instance(&mut memory, root.clone(), translator.clone());
    assert_eq!(warm.startup_cache_stats().unwrap().loaded, 1);
    let shader = register(&warm);
    assert_eq!(
        (shader.key.as_str(), &shader.words),
        (key.as_str(), &output.spirv)
    );
    assert_eq!(translator.0.load(Ordering::Relaxed), 0);
    assert_eq!(warm.startup_cache_stats().unwrap().hits, 1);
    let warm_ready = warm_start.elapsed();
    drop(warm);
    fs::remove_file(root.join(format!("{key}.sbc"))).unwrap();
    let cold_start = Instant::now();
    let cold = instance(&mut memory, root.clone(), translator.clone());
    assert_eq!(register(&cold).words, output.spirv);
    assert_eq!(translator.0.load(Ordering::Relaxed), 1);
    let cold_ready = cold_start.elapsed();
    println!(
        "translator CPU module readiness: cold_us={} warm_us={} translation_us_avoided={}",
        cold_ready.as_micros(),
        warm_ready.as_micros(),
        cold.startup_cache_stats().unwrap().translation_nanoseconds / 1000
    );
    drop(cold);
    // The translator's own reader must accept the record written by the runtime.
    let mut upstream = shadowbox::Cache::new(shadowbox::CacheConfig {
        memory: true,
        disk: Some(root.clone()),
    });
    let reread = upstream
        .translate_with_context(&input, Default::default(), || {
            panic!("runtime append must hit")
        })
        .unwrap();
    assert_eq!(reread.spirv, output.spirv);
    assert_eq!(upstream.stats.disk_hits, 1);
    let restarted = instance(&mut memory, root.clone(), translator.clone());
    register(&restarted);
    assert_eq!(translator.0.load(Ordering::Relaxed), 1);
    assert!(restarted.take_startup_cache_diagnostics().is_empty());
    drop(restarted);
    fs::remove_dir_all(root).unwrap();
}
