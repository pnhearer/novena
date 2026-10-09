//! Original CPU cache compatibility proof. Evidence: provenance 0031.
use novena::{
    api::{Object, ShaderTranslation},
    startup_cache::{StartupCacheConfig, TranslatedShader, TranslationStatus},
    Host, Instance, Registers,
};
use shadowbox::TargetGeneration;
#[path = "../support/startup_translator.rs"]
mod adapter;
use adapter::Adapter;
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
    let root = std::env::temp_dir().join(format!("startup-compatibility-{}", std::process::id()));
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
