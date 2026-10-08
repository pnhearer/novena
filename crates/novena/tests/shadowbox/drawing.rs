//! Original synthetic graphics programs. Public Mesa/envytools encodings, provenance 0027.
use novena::{functions, Host, Instance, Registers, Status};
use std::{ffi::c_void, sync::Mutex};

struct State {
    memory: Mutex<Vec<u8>>,
    frames: Mutex<Vec<(u32, u32, Vec<u8>)>>,
}

unsafe extern "C" fn read(user: *mut c_void, address: u64, out: *mut u8, size: u64) -> i32 {
    let state = unsafe { &*user.cast::<State>() };
    let memory = state.memory.lock().unwrap();
    let Some(end) = address.checked_add(size) else {
        return 1;
    };
    let Some(bytes) = memory.get(address as usize..end as usize) else {
        return 1;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
    }
    0
}

unsafe extern "C" fn write(user: *mut c_void, address: u64, data: *const u8, size: u64) -> i32 {
    let state = unsafe { &*user.cast::<State>() };
    let mut memory = state.memory.lock().unwrap();
    let Some(end) = address.checked_add(size) else {
        return 1;
    };
    let Some(bytes) = memory.get_mut(address as usize..end as usize) else {
        return 1;
    };
    unsafe {
        std::ptr::copy_nonoverlapping(data, bytes.as_mut_ptr(), bytes.len());
    }
    0
}

unsafe extern "C" fn present(
    user: *mut c_void,
    _window: u64,
    width: u32,
    height: u32,
    rgba: *const u8,
    stride: u64,
) {
    let state = unsafe { &*user.cast::<State>() };
    assert_eq!(stride, u64::from(width) * 4);
    let bytes = unsafe { std::slice::from_raw_parts(rgba, (stride * u64::from(height)) as usize) };
    state
        .frames
        .lock()
        .unwrap()
        .push((width, height, bytes.to_vec()));
}

fn call(instance: &Instance, name: &str, args: &[u64]) -> u64 {
    let mut registers = Registers::default();
    registers.x[..args.len()].copy_from_slice(args);
    assert_eq!(
        instance.call(functions::lookup(name).unwrap(), &mut registers),
        Status::Ok,
        "{name}"
    );
    registers.x[0]
}

fn put(state: &State, address: usize, bytes: &[u8]) {
    state.memory.lock().unwrap()[address..address + bytes.len()].copy_from_slice(bytes);
}

use novena::gpu::{graphics::FirstDrawContract, pipelines::CacheStats};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

const ALWAYS: u64 = 7 << 16;
const EXIT: u64 = 0xe300_0000_0000_0000 | ALWAYS;
const NOP: u64 = 0x50b0_0000_0000_0000 | ALWAYS;

fn mov(register: u8, value: f32) -> u64 {
    0x0100_0000_0000_0000 | ALWAYS | u64::from(register) | u64::from(value.to_bits()) << 20
}

fn shader(fragment: bool, green: f32) -> Vec<u8> {
    let mut header = [0_u32; 20];
    // Mesa sph.rs and public cla097sph.h: type/version/stage and component maps.
    header[0] = if fragment {
        2 | (3 << 5) | (5 << 10) | (1 << 14)
    } else {
        1 | (3 << 5) | (1 << 10)
    };
    let mut instructions = if fragment {
        header[18] = 15; // Four components at color target zero.
        vec![mov(0, 1.0), mov(1, green), mov(2, 0.5), mov(3, 1.0)]
    } else {
        header[6] = 15; // Generic input location zero, XYZW.
        header[12] = 15 << 28; // Position XYZW output at byte addresses 0x70..0x7c.
                               // Mesa SM50 ALD/AST: zero offset and vertex registers, four components.
        vec![
            0xefd8_0000_0000_0000 | ALWAYS | (255 << 8) | (0x80 << 20) | (255 << 39) | (3 << 47),
            0xeff0_0000_0000_0000
                | ALWAYS
                | (255 << 8)
                | (0x70 << 20)
                | (1 << 32)
                | (255 << 39)
                | (3 << 47),
        ]
    };
    instructions.push(EXIT);
    let mut bytes = vec![0; 0x30];
    bytes[..4].copy_from_slice(&0x12345678_u32.to_le_bytes());
    bytes.extend(header.into_iter().flat_map(u32::to_le_bytes));
    for bundle in instructions.chunks(3) {
        bytes.extend(0_u64.to_le_bytes());
        for slot in 0..3 {
            bytes.extend(bundle.get(slot).copied().unwrap_or(NOP).to_le_bytes());
        }
    }
    bytes
}

struct Translator(AtomicUsize);
impl novena::ShaderTranslator for Translator {
    fn translate(&self, _: novena::ShaderStage, program: &[u8]) -> Result<Vec<u32>, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let output = shadowbox::translate_header_prefixed(program).map_err(|e| e.to_string())?;
        if output.requires_subgroup_size_32 {
            return Err("subgroup size 32 is not enabled".into());
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/draw.spv");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            output
                .spirv
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let result = Command::new("spirv-val")
            .args(["--target-env", "vulkan1.2"])
            .arg(&path)
            .output()
            .unwrap();
        fs::remove_file(path).unwrap();
        assert!(
            result.status.success(),
            "SPIR-V validation: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(output.spirv)
    }
}

fn submit(instance: &Instance, state: &State, expected: Status) {
    let handle = call(instance, "nvnCommandBufferEndRecording", &[7]);
    put(state, 0x300, &handle.to_le_bytes());
    let mut r = Registers::default();
    r.x[..3].copy_from_slice(&[0, 1, 0x300]);
    assert_eq!(
        instance.call(functions::lookup("nvnQueueSubmitCommands").unwrap(), &mut r),
        expected
    );
}

fn wait_for_graphics(instance: &Instance) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while instance.graphics_pending_count().unwrap() != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "graphics worker timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(instance.take_graphics_cache_diagnostics().is_empty());
}

fn driver_file(root: &std::path::Path) -> PathBuf {
    let directory = fs::read_dir(root).unwrap().next().unwrap().unwrap().path();
    directory.join("driver.bin")
}

fn triangle(instance: &Instance, state: &State, address: u64, size: u64, primitive: u64) {
    put(state, 0x100, &4_u64.to_le_bytes());
    call(instance, "nvnCommandBufferBeginRecording", &[7]);
    call(
        instance,
        "nvnCommandBufferSetRenderTargets",
        &[7, 1, 0x100, 0, 0, 0],
    );
    call(instance, "nvnCommandBufferBindProgram", &[7, 12, 0xffff]); // Mask remains uninterpreted.
    call(
        instance,
        "nvnCommandBufferBindVertexBuffer",
        &[7, 0, address, size],
    );
    call(
        instance,
        "nvnCommandBufferBindVertexStreamState",
        &[7, 1, 20],
    );
    call(
        instance,
        "nvnCommandBufferBindVertexAttribState",
        &[7, 1, 21],
    );
    call(instance, "nvnCommandBufferBindColorState", &[7, 22]);
    call(instance, "nvnCommandBufferBindDepthStencilState", &[7, 23]);
    call(instance, "nvnCommandBufferBindPolygonState", &[7, 24]);
    call(instance, "nvnCommandBufferSetViewport", &[7, 0, 0, 64, 64]);
    call(instance, "nvnCommandBufferSetScissor", &[7, 0, 0, 64, 64]);
    call(
        instance,
        "nvnCommandBufferDrawArrays",
        &[7, primitive, 1, 3],
    );
}

fn check_pixels(pixels: &[u8], green: u8) {
    assert_eq!(pixels.len(), 64 * 64 * 4);
    let covered = [255, green, 128, 255];
    let clear = [0, 0, 255, 255];
    let pixel = |x: usize, y: usize| &pixels[(y * 64 + x) * 4..(y * 64 + x + 1) * 4];
    assert_eq!(pixel(32, 24), covered);
    for (x, y) in [(0, 0), (63, 0), (0, 63), (63, 63), (32, 60)] {
        assert_eq!(pixel(x, y), clear);
    }
    let mut count = 0;
    for y in 0..64 {
        for x in 0..64 {
            let p = pixel(x, y);
            assert!(
                p == clear || p == covered,
                "unexpected pixel at {x},{y}: {p:?}"
            );
            if p == covered {
                count += 1;
            }
            // Interior/exterior checks avoid the public Vulkan edge tie-breaking rule.
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let edges = [
                48.0 * (py - 8.0),
                -24.0 * (py - 8.0) - 48.0 * (px - 56.0),
                -24.0 * (py - 56.0) + 48.0 * (px - 32.0),
            ];
            if edges.iter().all(|e| *e > 48.0) {
                assert_eq!(p, covered, "interior {x},{y}");
            }
            if edges.iter().any(|e| *e < -48.0) {
                assert_eq!(p, clear, "exterior {x},{y}");
            }
        }
    }
    assert!(count > 1000 && count < 1300, "coverage count {count}");
}

#[test]
#[ignore = "requires Vulkan, the flat arena, spirv-val and Shadowbox; never skips"]
fn first_draw_executes_translated_triangle() {
    let state = Box::new(State {
        memory: Mutex::new(vec![0; 0x10000]),
        frames: Mutex::new(Vec::new()),
    });
    let host = Host {
        user: std::ptr::from_ref(&*state).cast_mut().cast(),
        read_memory: Some(read),
        write_memory: Some(write),
        present: Some(present),
        ..Host::default()
    };
    let instance = unsafe { Instance::with_host(host) };
    assert!(
        instance.set_first_draw_contract(Some(FirstDrawContract {
            triangle_list: 0xf001,
            float4: 0xf002,
            cull_none: 0xf003,
            rgba8: 0xf004,
            target_2d: 0xf005,
            identity_swizzle: [0; 4]
        })),
        "Vulkan must be active"
    );
    let inherited_cache = std::env::var_os("NOVENA_TEST_GRAPHICS_CACHE");
    let cache_root = inherited_cache
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tmp/graphics-cache-{}", std::process::id()))
        });
    let identity = novena::gpu::pipelines::TranslationIdentity {
        version: "synthetic-triangle-1".into(),
        configuration: "header-prefixed defaults; graphics interface 1".into(),
    };
    instance
        .set_graphics_pipeline_cache(&cache_root, &identity, 2, 8)
        .unwrap();
    assert_eq!(
        instance
            .graphics_persistence_stats()
            .unwrap()
            .driver_cache_loaded,
        inherited_cache.is_some()
    );
    // Reject invalid configuration without replacing the live cache.
    assert!(instance
        .set_graphics_pipeline_cache(&cache_root, &identity, 0, 8)
        .is_err());
    call(&instance, "nvnMemoryPoolBuilderSetDefaults", &[1]);
    call(
        &instance,
        "nvnMemoryPoolBuilderSetStorage",
        &[1, 0x1000, 0x5000],
    );
    call(&instance, "nvnMemoryPoolInitialize", &[2, 1]);
    let base = call(&instance, "nvnMemoryPoolGetBufferAddress", &[2]);
    assert_eq!(base, novena::global_memory::GUEST_BASE);
    call(&instance, "nvnTextureBuilderSetDefaults", &[3]);
    call(&instance, "nvnTextureBuilderSetSize2D", &[3, 64, 64]);
    call(&instance, "nvnTextureBuilderSetFormat", &[3, 0xf004]);
    call(&instance, "nvnTextureBuilderSetTarget", &[3, 0xf005]);
    call(&instance, "nvnTextureBuilderSetStorage", &[3, 2, 0x1000]);
    call(&instance, "nvnTextureInitialize", &[4, 3]);
    put(&state, 0x100, &4_u64.to_le_bytes());
    call(&instance, "nvnWindowBuilderSetDefaults", &[8]);
    call(&instance, "nvnWindowBuilderSetTextures", &[8, 1, 0x100]);
    call(&instance, "nvnWindowInitialize", &[9, 8]);
    call(&instance, "nvnCommandBufferInitialize", &[7, 0]);
    for (kind, address) in [
        ("VertexStreamState", 20),
        ("VertexAttribState", 21),
        ("ColorState", 22),
        ("DepthStencilState", 23),
        ("PolygonState", 24),
    ] {
        call(&instance, &format!("nvn{kind}SetDefaults"), &[address]);
    }
    call(&instance, "nvnVertexStreamStateSetStride", &[20, 32]);
    call(&instance, "nvnVertexStreamStateSetDivisor", &[20, 0]);
    call(&instance, "nvnVertexAttribStateSetFormat", &[21, 0xf002, 8]);
    call(&instance, "nvnVertexAttribStateSetStreamIndex", &[21, 0]);
    call(&instance, "nvnColorStateSetBlendEnable", &[22, 0, 0]);
    for setting in ["DepthTestEnable", "DepthWriteEnable", "StencilTestEnable"] {
        call(
            &instance,
            &format!("nvnDepthStencilStateSet{setting}"),
            &[23, 0],
        );
    }
    call(&instance, "nvnPolygonStateSetCullFace", &[24, 0xf003]);
    // Nonzero pool offset, first vertex, attribute offset, and padded stride.
    let vertices: [[f32; 4]; 3] = [
        [-0.75, -0.75, 0.0, 1.0],
        [0.75, -0.75, 0.0, 1.0],
        [0.0, 0.75, 0.0, 1.0],
    ];
    put(&state, 0x1080, &[0x5a; 128]);
    for (i, vertex) in vertices.into_iter().enumerate() {
        put(
            &state,
            0x1080 + (i + 1) * 32 + 8,
            &vertex
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    let translator = Arc::new(Translator(AtomicUsize::new(0)));
    instance.set_shader_translator(Some(translator.clone()));
    instance.set_shader_translation_enabled(true);
    // Count-two stride is the reader's existing 0x40 choice, not a new observation.
    put(&state, 0x1300, &shader(true, 0.25));
    put(&state, 0x1500, &shader(false, 0.0));
    put(&state, 0x400, &(base + 0x300).to_le_bytes());
    put(&state, 0x440, &(base + 0x500).to_le_bytes());
    call(&instance, "nvnProgramInitialize", &[12, 0]);
    call(&instance, "nvnProgramSetShaders", &[12, 2, 0x400]);
    assert_eq!(translator.0.load(Ordering::Relaxed), 2);
    let color = [0.0_f32, 0.0, 1.0, 1.0];
    put(
        &state,
        0x200,
        &color
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    call(&instance, "nvnCommandBufferBeginRecording", &[7]);
    call(
        &instance,
        "nvnCommandBufferSetRenderTargets",
        &[7, 1, 0x100, 0, 0, 0],
    );
    call(&instance, "nvnCommandBufferClearColor", &[7, 0, 0x200, 15]);
    submit(&instance, &state, Status::Ok);
    // The cold draw queues work without waiting; the next draw uses its ready result.
    triangle(&instance, &state, base + 0x80, 128, 0xf001);
    submit(&instance, &state, Status::Ok);
    wait_for_graphics(&instance);
    assert!(driver_file(&cache_root).is_file());
    for frame in 0..3 {
        triangle(&instance, &state, base + 0x80, 128, 0xf001);
        // Later edits must not change the state captured by an earlier binding.
        call(&instance, "nvnVertexStreamStateSetStride", &[20, 4]);
        submit(&instance, &state, Status::Ok);
        call(&instance, "nvnVertexStreamStateSetStride", &[20, 32]);
        call(&instance, "nvnQueuePresentTexture", &[0, 9, 0]);
        let frames = state.frames.lock().unwrap();
        assert_eq!((frames[frame].0, frames[frame].1), (64, 64));
        check_pixels(&frames[frame].2, 64);
        check_pixels(&state.memory.lock().unwrap()[0x2000..0x6000], 64);
    }
    assert_eq!(
        instance.graphics_cache_stats(),
        Some(CacheStats { hits: 3, misses: 1 })
    );
    assert_eq!(
        translator.0.load(Ordering::Relaxed),
        2,
        "queue uses retained translation"
    );
    triangle(&instance, &state, base + 0x80, 119, 0xf001);
    submit(&instance, &state, Status::BadArgument);
    triangle(&instance, &state, base + 0x80, 128, 5);
    submit(&instance, &state, Status::Unimplemented);
    call(&instance, "nvnColorStateSetBlendEnable", &[22, 0, 1]);
    triangle(&instance, &state, base + 0x80, 128, 0xf001);
    submit(&instance, &state, Status::Unimplemented);
    call(&instance, "nvnColorStateSetBlendEnable", &[22, 0, 0]);
    // Changed fragment content must miss the cache and change the actual image.
    put(&state, 0x1300, &shader(true, 0.75));
    call(&instance, "nvnProgramSetShaders", &[12, 2, 0x400]);
    triangle(&instance, &state, base + 0x80, 128, 0xf001);
    submit(&instance, &state, Status::Ok);
    wait_for_graphics(&instance);
    triangle(&instance, &state, base + 0x80, 128, 0xf001);
    // A later unsupported draw cannot discard an already completed draw's bytes.
    call(&instance, "nvnCommandBufferDrawArrays", &[7, 5, 1, 3]);
    submit(&instance, &state, Status::Unimplemented);
    call(&instance, "nvnQueuePresentTexture", &[0, 9, 0]);
    check_pixels(&state.frames.lock().unwrap().last().unwrap().2, 191);
    check_pixels(&state.memory.lock().unwrap()[0x2000..0x6000], 191);
    assert_eq!(
        instance.graphics_cache_stats(),
        Some(CacheStats { hits: 4, misses: 2 })
    );
    // Reopen the actual persistent graphics cache, then recover a damaged blob.
    instance
        .set_graphics_pipeline_cache(&cache_root, &identity, 2, 8)
        .unwrap();
    assert!(
        instance
            .graphics_persistence_stats()
            .unwrap()
            .driver_cache_loaded
    );
    triangle(&instance, &state, base + 0x80, 128, 0xf001);
    submit(&instance, &state, Status::Ok);
    wait_for_graphics(&instance);
    triangle(&instance, &state, base + 0x80, 128, 0xf001);
    submit(&instance, &state, Status::Ok);
    check_pixels(&state.memory.lock().unwrap()[0x2000..0x6000], 191);
    assert_eq!(
        instance.graphics_cache_stats(),
        Some(CacheStats { hits: 1, misses: 1 })
    );
    assert_eq!(
        translator.0.load(Ordering::Relaxed),
        4,
        "restart uses retained translations"
    );
    fs::write(driver_file(&cache_root), b"torn cache").unwrap();
    instance
        .set_graphics_pipeline_cache(&cache_root, &identity, 2, 8)
        .unwrap();
    assert!(
        !instance
            .graphics_persistence_stats()
            .unwrap()
            .driver_cache_loaded
    );
    assert!(!instance.take_graphics_cache_diagnostics().is_empty());
    triangle(&instance, &state, base + 0x80, 128, 0xf001);
    submit(&instance, &state, Status::Ok);
    wait_for_graphics(&instance);
    triangle(&instance, &state, base + 0x80, 128, 0xf001);
    submit(&instance, &state, Status::Ok);
    check_pixels(&state.memory.lock().unwrap()[0x2000..0x6000], 191);
    drop(instance);
    if inherited_cache.is_none() {
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "first_draw_executes_translated_triangle",
                "--include-ignored",
                "--nocapture",
            ])
            .env("NOVENA_TEST_GRAPHICS_CACHE", &cache_root)
            .status()
            .unwrap();
        assert!(result.success(), "graphics cache restart failed: {result}");
    }
    println!("MATCH translated triangle through async graphics workers, persistent cache restart and recovery, command recording, arena, presentation and readback");
}
