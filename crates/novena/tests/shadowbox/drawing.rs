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
    if name.get(3..) == Some("QueuePresentTexture") {
        assert_eq!(instance.poll_presentations(true), Status::Ok);
    }
    registers.x[0]
}

fn put(state: &State, address: usize, bytes: &[u8]) {
    state.memory.lock().unwrap()[address..address + bytes.len()].copy_from_slice(bytes);
}

use novena::gpu::{
    graphics::{FirstDrawContract, PrimitiveTopology, VertexFormat},
    pipelines::CacheStats,
};
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
    // Mesa sm50.rs:1938-1941 sets all four lane-mask bits for an immediate move.
    0x0100_0000_0000_0000
        | ALWAYS
        | (15 << 12)
        | u64::from(register)
        | u64::from(value.to_bits()) << 20
}

fn shader(fragment: bool, green: f32) -> Vec<u8> {
    shader_inputs(fragment, green, false)
}

fn shader_inputs(fragment: bool, green: f32, second: bool) -> Vec<u8> {
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
        header[6] = if second { 255 } else { 15 }; // One or two generic XYZW inputs.
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
    if second && !fragment {
        // Load the second input into R4..R7, then replace position W with R4.
        instructions.extend([
            0xefd8_0000_0000_0000
                | ALWAYS
                | 4
                | (255 << 8)
                | (0x90 << 20)
                | (255 << 39)
                | (3 << 47),
            0xeff0_0000_0000_0000
                | ALWAYS
                | 4
                | (255 << 8)
                | (0x7c << 20)
                | (1 << 32)
                | (255 << 39),
        ]);
    }
    pack_shader(header, instructions)
}

fn pack_shader(header: [u32; 20], mut instructions: Vec<u64>) -> Vec<u8> {
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
        let path = std::env::temp_dir().join(format!("draw-{}.spv", std::process::id()));
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
        Status::Ok
    );
    let finish = functions::all().find(|(_, name)| name.ends_with("QueueFinish")).unwrap().0;
    r.x[0] = 0;
    assert_eq!(instance.call(finish, &mut r), expected);

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
    triangle_state(instance, state, address, size);
    call(
        instance,
        "nvnCommandBufferDrawArrays",
        &[7, primitive, 1, 3],
    );
}

fn triangle_state(instance: &Instance, state: &State, address: u64, size: u64) {
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
}

fn indexed(
    instance: &Instance,
    state: &State,
    vertex_buffer: (u64, u64),
    index_type: u64,
    indices: u64,
    count: u64,
    base_vertex: i32,
) {
    triangle_state(instance, state, vertex_buffer.0, vertex_buffer.1);
    call(instance, "nvnCommandBufferClearColor", &[7, 0, 0x200, 15]);
    call(
        instance,
        "nvnCommandBufferDrawElementsBaseVertex",
        &[
            7,
            0xf001,
            index_type,
            count,
            indices,
            base_vertex as u32 as u64,
        ],
    );
}

fn indexed_proof(instance: &Instance, state: &State, base: u64, translator: &Translator) {
    for (i, vertex) in [
        [-0.75_f32, 0.75, 0.0, 1.0],
        [0.75, 0.75, 0.0, 1.0],
        [-0.75, -0.75, 0.0, 1.0],
    ]
    .into_iter()
    .enumerate()
    {
        put(
            state,
            0x1080 + (i + 4) * 32 + 8,
            &vertex
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    for (token, width) in [(0xf006, 2), (0xf007, 4)] {
        let mut cases = vec![(0, [3_u32, 1, 2]), (1, [2, 0, 1]), (-1, [4, 2, 3])];
        cases.push(if width == 2 {
            (-65532, [65535, 65533, 65534])
        } else {
            (-65535, [65538, 65536, 65537])
        });
        for (base_vertex, indices) in cases {
            let mut elements = vec![u32::MAX];
            elements.extend(indices);
            elements.push(u32::MAX);
            let bytes: Vec<_> = elements
                .into_iter()
                .flat_map(|i| i.to_le_bytes().into_iter().take(width))
                .collect();
            put(state, 0x1700, &bytes);
            // The address points after a poison prefix. No separate first-index argument exists.
            indexed(
                instance,
                state,
                (base + 0x80, 128),
                token,
                base + 0x700 + width as u64,
                3,
                base_vertex,
            );
            submit(instance, state, Status::Ok);
            call(instance, "nvnQueuePresentTexture", &[0, 9, 0]);
            check_pixels(&state.frames.lock().unwrap().last().unwrap().2, 64);
            check_pixels(&state.memory.lock().unwrap()[0x2000..0x6000], 64);
            println!("MATCH indexed width {width}, base vertex {base_vertex}, advanced address, count 3, presentation and arena pixels");
        }
        // Six elements consume two triangles. Trailing poison is outside the count.
        let bytes: Vec<_> = [3_u32, 1, 2, 4, 5, 6, u32::MAX]
            .into_iter()
            .flat_map(|i| i.to_le_bytes().into_iter().take(width))
            .collect();
        put(state, 0x1700, &bytes);
        indexed(
            instance,
            state,
            (base + 0x80, 224),
            token,
            base + 0x700,
            6,
            0,
        );
        submit(instance, state, Status::Ok);
        call(instance, "nvnQueuePresentTexture", &[0, 9, 0]);
        for pixels in [
            &state.frames.lock().unwrap().last().unwrap().2[..],
            &state.memory.lock().unwrap()[0x2000..0x6000],
        ] {
            let pixel = |x: usize, y: usize| &pixels[(y * 64 + x) * 4..(y * 64 + x + 1) * 4];
            assert_eq!(pixel(32, 24), [255, 64, 128, 255]);
            assert_eq!(
                pixel(16, 48),
                [255, 64, 128, 255],
                "second triangle consumes indices four through six"
            );
            assert_eq!(pixel(0, 0), [0, 0, 255, 255]);
        }
        // The last element must fit its pool, and each selected attribute must fit its binding.
        indexed(
            instance,
            state,
            (base + 0x80, 128),
            token,
            base + 0x30000 - width as u64,
            2,
            0,
        );
        submit(instance, state, Status::BadArgument);
        indexed(
            instance,
            state,
            (base + 0x80, 128),
            token,
            base + 0x701,
            3,
            0,
        );
        submit(instance, state, Status::BadArgument);
        indexed(
            instance,
            state,
            (base + 0x80, 119),
            token,
            base + 0x700,
            3,
            0,
        );
        submit(instance, state, Status::BadArgument);
        indexed(
            instance,
            state,
            (base + 0x80, 128),
            token,
            base + 0x700,
            3,
            -4,
        );
        submit(instance, state, Status::BadArgument);
        indexed(
            instance,
            state,
            (base + 0x80, 128),
            token,
            base + 0x700,
            3,
            i32::MAX,
        );
        submit(instance, state, Status::BadArgument);
        indexed(instance, state, (base + 0x80, 128), token, 0, 0, -1);
        submit(instance, state, Status::Ok);
        assert!(state.memory.lock().unwrap()[0x2000..0x6000]
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 255, 255]));
    }
    indexed(instance, state, (base + 0x80, 128), 1, base + 0x700, 3, 0);
    submit(instance, state, Status::Unimplemented);
    indexed(instance, state, (base + 0x80, 128), 0xf006, base - 1, 3, 0);
    submit(instance, state, Status::BadArgument);
    assert_eq!(
        translator.0.load(Ordering::Relaxed),
        2,
        "indexed draws reuse retained stages"
    );
    assert_eq!(
        instance.graphics_cache_stats().unwrap().misses,
        1,
        "index type and base vertex are dynamic"
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
    draw_proof(false);
}

#[test]
#[ignore = "requires Vulkan, the flat arena, spirv-val and Shadowbox; never skips"]
fn indexed_draw_executes_translated_triangle() {
    draw_proof(true);
}

fn draw_proof(indexed_draw: bool) {
    let Fixture {
        state,
        instance,
        base,
    } = Fixture::new(contract());
    let inherited_cache = std::env::var_os("NOVENA_TEST_GRAPHICS_CACHE");
    let cache_root = inherited_cache
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("graphics-cache-{}", std::process::id()))
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
    if indexed_draw {
        indexed_proof(&instance, &state, base, &translator);
        return;
    }
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

struct Fixture {
    instance: Instance,
    state: Box<State>,
    base: u64,
}

impl Fixture {
    fn new(contract: FirstDrawContract) -> Self {
        Self::with_instance(contract, |host| unsafe { Instance::with_host(host) })
    }

    fn with_instance(contract: FirstDrawContract, build: impl FnOnce(Host) -> Instance) -> Self {
        let state = Box::new(State {
            memory: Mutex::new(vec![0; 0x40000]),
            frames: Mutex::new(Vec::new()),
        });
        let host = Host {
            user: std::ptr::from_ref(&*state).cast_mut().cast(),
            read_memory: Some(read),
            write_memory: Some(write),
            present: Some(present),
            ..Host::default()
        };
        let instance = build(host);
        assert!(
            instance.set_first_draw_contract(Some(contract)),
            "Vulkan must be active"
        );
        call(&instance, "nvnMemoryPoolBuilderSetDefaults", &[1]);
        call(
            &instance,
            "nvnMemoryPoolBuilderSetStorage",
            &[1, 0x1000, 0x30000],
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
            call(
                &instance,
                &format!(
                    "{}{kind}SetDefaults",
                    &functions::all().next().unwrap().1[..3]
                ),
                &[address],
            );
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
        Self {
            instance,
            state,
            base,
        }
    }

    fn program(
        &self,
        translator: Arc<dyn novena::ShaderTranslator>,
        fragment: &[u8],
        vertex: &[u8],
    ) {
        self.instance.set_shader_translator(Some(translator));
        self.instance.set_shader_translation_enabled(true);
        put(&self.state, 0x1300, fragment);
        put(&self.state, 0x1500, vertex);
        put(&self.state, 0x400, &(self.base + 0x300).to_le_bytes());
        put(&self.state, 0x440, &(self.base + 0x500).to_le_bytes());
        call(&self.instance, "nvnProgramInitialize", &[12, 0]);
        call(&self.instance, "nvnProgramSetShaders", &[12, 2, 0x400]);
    }

    fn begin(&self) {
        let i = &self.instance;
        call(i, "nvnCommandBufferBeginRecording", &[7]);
        call(
            i,
            "nvnCommandBufferSetRenderTargets",
            &[7, 1, 0x100, 0, 0, 0],
        );
        put(&self.state, 0x200, &floats(&[0.0, 0.0, 1.0, 1.0]));
        call(i, "nvnCommandBufferClearColor", &[7, 0, 0x200, 15]);
        call(i, "nvnCommandBufferBindProgram", &[7, 12, 0xffff]);
        for (kind, address) in [
            ("ColorState", 22),
            ("DepthStencilState", 23),
            ("PolygonState", 24),
        ] {
            call(i, &format!("nvnCommandBufferBind{kind}"), &[7, address]);
        }
        call(i, "nvnCommandBufferSetViewport", &[7, 0, 0, 64, 64]);
        call(i, "nvnCommandBufferSetScissor", &[7, 0, 0, 64, 64]);
    }

    fn pixels(&self) -> Vec<u8> {
        call(&self.instance, "nvnQueuePresentTexture", &[0, 9, 0]);
        let frames = self.state.frames.lock().unwrap();
        let (w, h, pixels) = frames.last().unwrap();
        assert_eq!((*w, *h), (64, 64));
        assert_eq!(pixels, &self.state.memory.lock().unwrap()[0x2000..0x6000]);
        pixels.clone()
    }
}

fn contract() -> FirstDrawContract {
    FirstDrawContract {
        topologies: vec![(0xf001, PrimitiveTopology::TriangleList)],
        attribute_formats: vec![(0xf002, VertexFormat::Float4)],
        attribute_state_stride: None,
        stream_state_stride: None,
        cull_none: 0xf003,
        rgba8: 0xf004,
        target_2d: 0xf005,
        identity_swizzle: [0; 4],
        index_u16: 0xf006,
        index_u32: 0xf007,
        depth_raster: None,
    }
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn check_geometry(pixels: &[u8], vertices: &[[f32; 2]], triangles: &[[usize; 3]], color: [u8; 4]) {
    let clear = [0, 0, 255, 255];
    let mut checked = [0; 2];
    for y in 0..64 {
        for x in 0..64 {
            let point = [x as f32 + 0.5, y as f32 + 0.5];
            let mut covered = false;
            let mut boundary = false;
            for &[a, b, c] in triangles {
                let tri = [vertices[a], vertices[b], vertices[c]];
                let sign = edge(tri[0], tri[1], tri[2]).signum();
                let edges = [0, 1, 2].map(|i| {
                    let a = tri[i];
                    let b = tri[(i + 1) % 3];
                    edge(a, b, point) * sign / ((b[0] - a[0]).hypot(b[1] - a[1]))
                });
                covered |= edges.iter().all(|e| *e > 0.0);
                boundary |=
                    edges.iter().all(|e| *e >= -1.0) && edges.iter().any(|e| e.abs() <= 1.0);
            }
            let pixel = &pixels[(y * 64 + x) * 4..(y * 64 + x + 1) * 4];
            let near = |expected: &[u8]| {
                pixel
                    .iter()
                    .zip(expected)
                    .all(|(&a, &b)| a.abs_diff(b) <= 1)
            };
            assert!(
                pixel == clear || near(&color),
                "unexpected color at {x},{y}: {pixel:?}"
            );
            if !boundary {
                let expected = if covered { &color } else { &clear };
                assert!(
                    near(expected),
                    "coverage at {x},{y}: {pixel:?}, expected {expected:?}"
                );
                checked[usize::from(covered)] += 1;
            }
        }
    }
    assert!(
        checked.iter().all(|&n| n > 100),
        "insufficient geometry checks: {checked:?}"
    );
}

#[test]
#[ignore = "requires Vulkan, spirv-val and Shadowbox; never skips"]
fn primitive_topologies_read_back_pixels() {
    let mut c = contract();
    // Observed values are experimental hypotheses; the fan token is synthetic. 0028.
    c.topologies = vec![
        (4, PrimitiveTopology::TriangleList),
        (5, PrimitiveTopology::TriangleStrip),
        (0xf006, PrimitiveTopology::TriangleFan),
    ];
    let f = Fixture::new(c.clone());
    let translator = Arc::new(Translator(AtomicUsize::new(0)));
    f.program(translator.clone(), &shader(true, 0.25), &shader(false, 0.0));
    let vertices = [
        [8.0, 8.0],
        [56.0, 8.0],
        [12.0, 48.0],
        [52.0, 56.0],
        [28.0, 24.0],
        [44.0, 36.0],
    ];
    put(&f.state, 0x1080, &[0x5a; 224]);
    for (i, &[x, y]) in vertices.iter().enumerate() {
        put(
            &f.state,
            0x1080 + (i + 1) * 32 + 8,
            &floats(&[x / 32.0 - 1.0, y / 32.0 - 1.0, 0.0, 1.0]),
        );
    }
    for count in [4, 6] {
        let mut masks = Vec::new();
        for &(token, topology) in &c.topologies {
            let triangles: Vec<_> = match topology {
                PrimitiveTopology::TriangleList => (0..count / 3)
                    .map(|n| [n * 3, n * 3 + 1, n * 3 + 2])
                    .collect(),
                PrimitiveTopology::TriangleStrip => {
                    (0..count - 2).map(|n| [n, n + 1, n + 2]).collect()
                }
                PrimitiveTopology::TriangleFan => (1..count - 1).map(|n| [0, n, n + 1]).collect(),
            };
            for repeat in 0..if count == 4 { 3 } else { 2 } {
                f.begin();
                call(
                    &f.instance,
                    "nvnCommandBufferBindVertexBuffer",
                    &[7, 0, f.base + 0x80, 224],
                );
                call(
                    &f.instance,
                    "nvnCommandBufferBindVertexStreamState",
                    &[7, 1, 20],
                );
                call(
                    &f.instance,
                    "nvnCommandBufferBindVertexAttribState",
                    &[7, 1, 21],
                );
                call(
                    &f.instance,
                    "nvnCommandBufferDrawArrays",
                    &[7, u64::from(token), 1, count as u64],
                );
                submit(&f.instance, &f.state, Status::Ok);
                if count == 4 && repeat == 0 {
                    wait_for_graphics(&f.instance);
                    continue;
                }
                let pixels = f.pixels();
                check_geometry(&pixels, &vertices, &triangles, [255, 64, 128, 255]);
                masks.push(pixels);
            }
            println!("MATCH topology {token:#x} -> {topology:?}, {count} vertices, canonical and presented pixels");
        }
        assert_ne!(
            masks[0], masks[2],
            "list and strip must have different coverage"
        );
        assert_ne!(
            masks[2], masks[4],
            "strip and fan must have different coverage"
        );
        assert_ne!(
            masks[0], masks[4],
            "list and fan must have different coverage"
        );
    }
    assert_eq!(translator.0.load(Ordering::Relaxed), 2);
    assert_eq!(
        f.instance.graphics_cache_stats(),
        Some(CacheStats {
            hits: 12,
            misses: 3
        })
    );
    // A real translation of the second input changes position W and image coverage.
    c.attribute_state_stride = Some(32);
    c.stream_state_stride = Some(16);
    assert!(f.instance.set_first_draw_contract(Some(c.clone())));
    f.program(
        translator.clone(),
        &shader(true, 0.25),
        &shader_inputs(false, 0.0, true),
    );
    call(&f.instance, "nvnVertexStreamStateSetDefaults", &[36]);
    call(&f.instance, "nvnVertexStreamStateSetStride", &[36, 20]);
    call(&f.instance, "nvnVertexStreamStateSetDivisor", &[36, 0]);
    call(&f.instance, "nvnVertexAttribStateSetDefaults", &[53]);
    call(
        &f.instance,
        "nvnVertexAttribStateSetFormat",
        &[53, 0xf002, 4],
    );
    call(&f.instance, "nvnVertexAttribStateSetStreamIndex", &[53, 1]);
    put(&f.state, 0x1800, &[0x5a; 160]);
    for i in 1..=6 {
        put(
            &f.state,
            0x1800 + i * 20 + 4,
            &floats(&[2.0, 3.0, 4.0, 5.0]),
        );
    }
    let shrunk = vertices.map(|[x, y]| [32.0 + (x - 32.0) / 2.0, 32.0 + (y - 32.0) / 2.0]);
    let triangles: Vec<_> = (0..4).map(|n| [n, n + 1, n + 2]).collect();
    for (repeat, size) in [140, 140, 140, 139].into_iter().enumerate() {
        f.begin();
        call(
            &f.instance,
            "nvnCommandBufferBindVertexBuffer",
            &[7, 1, f.base + 0x800, size],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindVertexBuffer",
            &[7, 0, f.base + 0x80, 224],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindVertexStreamState",
            &[7, 2, 20],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindVertexAttribState",
            &[7, 2, 21],
        );
        call(&f.instance, "nvnCommandBufferDrawArrays", &[7, 5, 1, 6]);
        submit(
            &f.instance,
            &f.state,
            if size == 139 {
                Status::BadArgument
            } else {
                Status::Ok
            },
        );
        if repeat == 0 {
            wait_for_graphics(&f.instance);
        } else if size == 140 {
            check_geometry(&f.pixels(), &shrunk, &triangles, [255, 64, 128, 255]);
        }
    }
    assert_eq!(translator.0.load(Ordering::Relaxed), 4);
    assert_eq!(
        f.instance.graphics_cache_stats(),
        Some(CacheStats {
            hits: 14,
            misses: 4
        })
    );
    println!("MATCH translated second attribute in stream one changes homogeneous position and pixel coverage; short stream rejected");
    c.topologies.push((5, PrimitiveTopology::TriangleFan));
    assert!(f.instance.set_first_draw_contract(Some(c)));
    triangle(&f.instance, &f.state, f.base + 0x80, 224, 5);
    submit(&f.instance, &f.state, Status::Unimplemented);
}

struct SourceShaders {
    fragment: Vec<u32>,
    vertex: Vec<u32>,
}

impl novena::ShaderTranslator for SourceShaders {
    fn translate(&self, _: novena::ShaderStage, bytes: &[u8]) -> Result<Vec<u32>, String> {
        match bytes.first() {
            Some(0x62) => Ok(self.fragment.clone()),
            Some(0x61) => Ok(self.vertex.clone()),
            _ => Err("unknown original shader marker".into()),
        }
    }
}

fn compile_source(stage: &str, source: &str) -> Vec<u32> {
    let dir = std::env::temp_dir().join(format!("draw-source-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let input = dir.join(format!("format.{stage}"));
    let output = dir.join(format!("format.{stage}.spv"));
    fs::write(&input, source).unwrap();
    let result = Command::new("glslangValidator")
        .args(["-V", "--target-env", "vulkan1.2", "-o"])
        .arg(&output)
        .arg(&input)
        .output()
        .expect("glslangValidator must be available");
    assert!(
        result.status.success(),
        "compile shader: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new("spirv-val")
        .args(["--target-env", "vulkan1.2"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "validate shader: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = fs::read(output).unwrap();
    fs::remove_dir_all(dir).unwrap();
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect()
}

struct FormatCase {
    format: VertexFormat,
    bytes: Vec<u8>,
    decoded: [f32; 4],
    alignment: usize,
}

fn format_cases() -> Vec<FormatCase> {
    let f = [-1.0, 0.25, 0.5, 1.0];
    let mut cases = Vec::new();
    for (n, format) in [
        VertexFormat::Float,
        VertexFormat::Float2,
        VertexFormat::Float3,
        VertexFormat::Float4,
    ]
    .into_iter()
    .enumerate()
    {
        let mut decoded = [0.0, 0.0, 0.0, 1.0];
        decoded[..n + 1].copy_from_slice(&f[..n + 1]);
        cases.push(FormatCase {
            format,
            bytes: floats(&f[..n + 1]),
            decoded,
            alignment: 4,
        });
    }
    for (n, format) in [(2, VertexFormat::Half2), (4, VertexFormat::Half4)] {
        let mut decoded = [0.0, 0.0, 0.0, 1.0];
        decoded[..n].copy_from_slice(&f[..n]);
        cases.push(FormatCase {
            format,
            bytes: [0xbc00_u16, 0x3400, 0x3800, 0x3c00][..n]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
            decoded,
            alignment: 2,
        });
    }
    cases.push(FormatCase {
        format: VertexFormat::Unorm8x4,
        bytes: vec![0, 64, 128, 255],
        decoded: [0.0, 64.0 / 255.0, 128.0 / 255.0, 1.0],
        alignment: 1,
    });
    cases.push(FormatCase {
        format: VertexFormat::Snorm8x4,
        bytes: vec![128, 32, 64, 127],
        decoded: [-1.0, 32.0 / 127.0, 64.0 / 127.0, 1.0],
        alignment: 1,
    });
    for (n, format, signed) in [
        (2, VertexFormat::Unorm16x2, false),
        (4, VertexFormat::Unorm16x4, false),
        (2, VertexFormat::Snorm16x2, true),
        (4, VertexFormat::Snorm16x4, true),
    ] {
        let (words, values) = if signed {
            (
                [0x8000_u16, 8192, 16384, 32767],
                [-1.0, 8192.0 / 32767.0, 16384.0 / 32767.0, 1.0],
            )
        } else {
            (
                [0_u16, 16384, 32768, 65535],
                [0.0, 16384.0 / 65535.0, 32768.0 / 65535.0, 1.0],
            )
        };
        let mut decoded = [0.0, 0.0, 0.0, 1.0];
        decoded[..n].copy_from_slice(&values[..n]);
        cases.push(FormatCase {
            format,
            bytes: words[..n].iter().flat_map(|v| v.to_le_bytes()).collect(),
            decoded,
            alignment: 2,
        });
    }
    cases
}

#[test]
#[ignore = "requires Vulkan, glslangValidator and spirv-val; never skips"]
fn vertex_formats_read_back_pixels() {
    let shaders = Arc::new(SourceShaders {
        vertex: compile_source(
            "vert",
            r#"#version 450
layout(location=0) in vec4 position;
layout(location=1) in vec4 value;
layout(location=2) out vec4 shade;
void main() { gl_Position = position; shade = value; }
"#,
        ),
        fragment: compile_source(
            "frag",
            r#"#version 450
layout(location=2) in vec4 shade;
layout(location=0) out vec4 color;
void main() { color = shade * 0.25 + 0.5; }
"#,
        ),
    });
    let cases = format_cases();
    let mut c = contract();
    c.attribute_state_stride = Some(32);
    c.stream_state_stride = Some(16);
    c.attribute_formats.extend(
        cases
            .iter()
            .enumerate()
            .map(|(i, case)| (0xf100 + i as u64, case.format)),
    );
    let f = Fixture::new(c.clone());
    f.program(shaders, &shader(true, 0.0), &shader(false, 0.0));
    let positions = [[8.0, 8.0], [56.0, 8.0], [32.0, 56.0]];
    for stream in 0..4 {
        for setting in ["Defaults", "Divisor"] {
            call(
                &f.instance,
                &format!("nvnVertexStreamStateSet{setting}"),
                &[20 + stream * 16, 0],
            );
        }
    }
    call(&f.instance, "nvnVertexAttribStateSetDefaults", &[53]);
    for (index, case) in cases.iter().enumerate() {
        let expected = case
            .decoded
            .map(|v| ((v * 0.25 + 0.5) * 255.0).round() as u8);
        for (separate, constant) in [(false, false), (true, false), (true, true)] {
            let stream = if separate { index as u64 % 3 + 1 } else { 0 };
            let stride = if separate {
                32
            } else {
                36 + index as u64 % 4 * 4
            };
            let color_offset = if separate {
                case.alignment
            } else {
                24 + case.alignment
            };
            let color_stride = if constant {
                0
            } else if separate {
                (color_offset + case.bytes.len() + case.alignment) as u64
            } else {
                stride
            };
            let vertex_size = 3 * stride + 8 + 16;
            let color_size = 3 * color_stride + color_offset as u64 + case.bytes.len() as u64;
            put(&f.state, 0x1080, &[0x5a; 256]);
            put(&f.state, 0x1800, &[0x5a; 256]);
            for (i, &[x, y]) in positions.iter().enumerate() {
                put(
                    &f.state,
                    0x1080 + (i + 1) * stride as usize + 8,
                    &floats(&[x / 32.0 - 1.0, y / 32.0 - 1.0, 0.0, 1.0]),
                );
                let base = if separate { 0x1800 } else { 0x1080 };
                put(
                    &f.state,
                    base + (i + 1) * color_stride as usize + color_offset,
                    &case.bytes,
                );
            }
            call(&f.instance, "nvnVertexStreamStateSetStride", &[20, stride]);
            call(
                &f.instance,
                "nvnVertexStreamStateSetStride",
                &[20 + stream * 16, color_stride],
            );
            call(
                &f.instance,
                "nvnVertexAttribStateSetFormat",
                &[53, 0xf100 + index as u64, color_offset as u64],
            );
            call(
                &f.instance,
                "nvnVertexAttribStateSetStreamIndex",
                &[53, stream],
            );
            for repeat in 0..4 {
                f.begin();
                if separate {
                    // Bind this first to prove a subsequent stream-zero bind preserves it.
                    call(
                        &f.instance,
                        "nvnCommandBufferBindVertexBuffer",
                        &[
                            7,
                            stream,
                            f.base + 0x800,
                            color_size - u64::from(repeat == 3),
                        ],
                    );
                }
                let size = if separate {
                    vertex_size
                } else {
                    vertex_size.max(color_size)
                };
                call(
                    &f.instance,
                    "nvnCommandBufferBindVertexBuffer",
                    &[
                        7,
                        0,
                        f.base + 0x80,
                        size - u64::from(!separate && repeat == 3),
                    ],
                );
                call(
                    &f.instance,
                    "nvnCommandBufferBindVertexStreamState",
                    &[7, stream + 1, 20],
                );
                call(
                    &f.instance,
                    "nvnCommandBufferBindVertexAttribState",
                    &[7, 2, 21],
                );
                call(
                    &f.instance,
                    "nvnCommandBufferDrawArrays",
                    &[7, 0xf001, 1, 3],
                );
                call(
                    &f.instance,
                    "nvnVertexAttribStateSetFormat",
                    &[53, 0xf100 + index as u64, 0],
                );
                call(
                    &f.instance,
                    "nvnVertexStreamStateSetStride",
                    &[20 + stream * 16, 0],
                );
                submit(
                    &f.instance,
                    &f.state,
                    if repeat == 3 {
                        Status::BadArgument
                    } else {
                        Status::Ok
                    },
                );
                if repeat == 0 {
                    wait_for_graphics(&f.instance);
                } else if repeat < 3 {
                    check_geometry(&f.pixels(), &positions, &[[0, 1, 2]], expected);
                }
                call(
                    &f.instance,
                    "nvnVertexAttribStateSetFormat",
                    &[53, 0xf100 + index as u64, color_offset as u64],
                );
                call(
                    &f.instance,
                    "nvnVertexStreamStateSetStride",
                    &[20 + stream * 16, color_stride],
                );
            }
            println!("MATCH {:?}, stream {stream}, stride {color_stride}, offset {color_offset}, pixels {expected:?}; short range rejected", case.format);
        }
    }
    assert_eq!(
        f.instance.graphics_cache_stats(),
        Some(CacheStats {
            hits: 72,
            misses: 36
        })
    );
    // An observed guest format has no automatic mapping in this experiment.
    let record = |vertex_offset, vertex_size, color_offset, bind_color| {
        f.begin();
        call(
            &f.instance,
            "nvnCommandBufferBindVertexBuffer",
            &[7, 0, f.base + vertex_offset, vertex_size],
        );
        if bind_color {
            call(
                &f.instance,
                "nvnCommandBufferBindVertexBuffer",
                &[7, 3, f.base + color_offset, 256],
            );
        }
        call(
            &f.instance,
            "nvnCommandBufferBindVertexStreamState",
            &[7, 4, 20],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindVertexAttribState",
            &[7, 2, 21],
        );
        call(
            &f.instance,
            "nvnCommandBufferDrawArrays",
            &[7, 0xf001, 1, 3],
        );
    };
    call(&f.instance, "nvnVertexAttribStateSetFormat", &[53, 0x2e, 2]);
    record(0x80, 256, 0x800, true);
    submit(&f.instance, &f.state, Status::Unimplemented);
    call(
        &f.instance,
        "nvnVertexAttribStateSetFormat",
        &[53, 0xf10b, 2],
    );
    record(0x80, 119, 0x800, true);
    submit(&f.instance, &f.state, Status::BadArgument);
    record(0x81, 256, 0x800, true);
    submit(&f.instance, &f.state, Status::BadArgument);
    record(0x80, 256, 0x801, true);
    submit(&f.instance, &f.state, Status::BadArgument);
    record(0x80, 256, 0x800, false);
    submit(&f.instance, &f.state, Status::Unimplemented);
    let divisor_name = functions::all()
        .find(|(_, name)| name.get(3..) == Some("VertexStreamStateSetDivisor"))
        .unwrap().1;
    call(&f.instance, divisor_name, &[68, 2]);
    record(0x80, 256, 0x800, true);
    submit(&f.instance, &f.state, Status::Unimplemented);
    call(&f.instance, "nvnVertexStreamStateSetDivisor", &[68, 0]);
    c.attribute_formats.push((0xf10b, VertexFormat::Float4));
    assert!(f.instance.set_first_draw_contract(Some(c.clone())));
    record(0x80, 256, 0x800, true);
    submit(&f.instance, &f.state, Status::Unimplemented);
    c.attribute_formats.pop();
    c.attribute_state_stride = None;
    assert!(f.instance.set_first_draw_contract(Some(c.clone())));
    record(0x80, 256, 0x800, true);
    submit(&f.instance, &f.state, Status::Unimplemented);
    c.attribute_state_stride = Some(32);
    c.stream_state_stride = None;
    assert!(f.instance.set_first_draw_contract(Some(c)));
    record(0x80, 256, 0x800, true);
    submit(&f.instance, &f.state, Status::Unimplemented);
}

fn uniform_shader(fragment: bool) -> Vec<u8> {
    let mut header = [0_u32; 20];
    header[0] = if fragment {
        2 | (3 << 5) | (5 << 10) | (1 << 14)
    } else {
        1 | (3 << 5) | (1 << 10)
    };
    // Mesa OpMov: bank in bits 34..39, four-byte offset in bits 20..34.
    let cbuf = |register: u64, offset: u64| {
        0x4c98_0000_0000_0000 | ALWAYS | register | ((offset / 4) << 20) | (2 << 34) | (15 << 39)
    };
    let instructions = if fragment {
        header[18] = 15;
        (0..4).map(|reg| cbuf(reg, 16 + reg * 4)).collect()
    } else {
        header[6] = 15;
        header[12] = 15 << 28;
        vec![
            0xefd8_0000_0000_0000 | ALWAYS | (255 << 8) | (0x80 << 20) | (255 << 39) | (3 << 47),
            cbuf(3, 0),
            0xeff0_0000_0000_0000
                | ALWAYS
                | (255 << 8)
                | (0x70 << 20)
                | (1 << 32)
                | (255 << 39)
                | (3 << 47),
        ]
    };
    pack_shader(header, instructions)
}

#[test]
#[ignore = "requires Vulkan, the flat arena, spirv-val and Shadowbox; never skips"]
fn uniform_banks_colour_two_draws() {
    use novena::gpu::uniforms::{UniformBankMapping, UniformBufferContract, UniformStage};
    let f = Fixture::new(contract());
    let translator = Arc::new(Translator(AtomicUsize::new(0)));
    f.program(
        translator.clone(),
        &uniform_shader(true),
        &uniform_shader(false),
    );
    let vertices = [
        [-0.75, -0.75, 0.0, 1.0],
        [0.75, -0.75, 0.0, 1.0],
        [0.0, 0.75, 0.0, 1.0],
    ];
    put(&f.state, 0x1080, &[0x5a; 128]);
    for (i, vertex) in vertices.iter().enumerate() {
        put(&f.state, 0x1080 + (i + 1) * 32 + 8, &floats(vertex));
    }
    let Fixture {
        state,
        instance,
        base,
    } = f;
    let mappings = vec![
        UniformBankMapping {
            stage: 0,
            index: 1,
            target: UniformStage::Vertex,
            bank: 2,
        },
        UniformBankMapping {
            stage: 1,
            index: 0,
            target: UniformStage::Fragment,
            bank: 2,
        },
    ];
    instance
        .set_uniform_buffer_contract(UniformBufferContract {
            bindings: mappings.clone(),
            storage_buffers: false,
        })
        .unwrap();
    // Duplicate sources or targets must fail without replacing the live mapping.
    let mut duplicates = mappings.clone();
    duplicates.push(mappings[0]);
    assert!(instance
        .set_uniform_buffer_contract(UniformBufferContract {
            bindings: duplicates,
            storage_buffers: false
        })
        .is_err());
    let context = novena::gpu::Context::new().expect("Vulkan uniform proof");
    let properties = unsafe {
        context
            .instance
            .get_physical_device_properties(context.physical_device)
    };
    println!(
        "uniform limit {}, storage limit {}, uniform alignment {}, storage alignment {}",
        properties.limits.max_uniform_buffer_range,
        properties.limits.max_storage_buffer_range,
        properties.limits.min_uniform_buffer_offset_alignment,
        properties.limits.min_storage_buffer_offset_alignment
    );
    let alignment = properties
        .limits
        .min_uniform_buffer_offset_alignment
        .max(properties.limits.min_storage_buffer_offset_alignment)
        .max(16);
    let align = |offset: u64| offset.div_ceil(alignment) * alignment;
    let vertex = align(0x8000);
    let red = align(vertex + 0x1000);
    let green = align(red + 0x1000);
    let full = align(green + 0x1000);
    assert!(full + 65536 <= 0x30000);
    // A decoy at bank offset zero catches lost byte offsets.
    for (offset, value) in [(vertex, 1.0_f32), (red, 0.0), (green, 0.0)] {
        put(&state, (0x1000 + offset) as usize, &value.to_le_bytes());
    }
    for (offset, colour) in [
        (red, [1.0_f32, 0.25, 0.5, 1.0]),
        (green, [1.0, 0.75, 0.5, 1.0]),
        (full, [1.0, 0.75, 0.5, 1.0]),
    ] {
        put(
            &state,
            (0x1000 + offset + 16) as usize,
            &colour
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    // Clear to blue, matching the triangle proof's uncovered pixels.
    let clear = [0.0_f32, 0.0, 1.0, 1.0];
    put(
        &state,
        0x200,
        &clear
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
    let begin = || triangle_state(&instance, &state, base + 0x80, 128);
    let bind = |offset, size| {
        call(
            &instance,
            "nvnCommandBufferBindUniformBuffer",
            &[7, 0, 1, base + vertex, 16],
        );
        call(
            &instance,
            "nvnCommandBufferBindUniformBuffer",
            &[7, 1, 0, base + offset, size],
        );
        call(&instance, "nvnCommandBufferDrawArrays", &[7, 0xf001, 1, 3]);
    };
    begin();
    call(&instance, "nvnCommandBufferDrawArrays", &[7, 0xf001, 1, 3]);
    submit(&instance, &state, Status::Unimplemented);
    begin();
    bind(red, 32);
    call(
        &instance,
        "nvnCommandBufferBindUniformBuffer",
        &[7, 5, 0, base + red, 32],
    );
    call(&instance, "nvnCommandBufferDrawArrays", &[7, 0xf001, 1, 3]);
    submit(&instance, &state, Status::Unimplemented);
    wait_for_graphics(&instance);
    for storage in [false, true] {
        instance
            .set_uniform_buffer_contract(UniformBufferContract {
                bindings: mappings.clone(),
                storage_buffers: storage,
            })
            .unwrap();
        begin();
        bind(red, 32);
        submit(&instance, &state, Status::Ok);
        wait_for_graphics(&instance);
        // Both draws in one recording must keep their own descriptor contents.
        begin();
        bind(red, 32);
        call(
            &instance,
            "nvnCommandBufferBindUniformBuffer",
            &[7, 1, 0, base + green, 32],
        );
        call(&instance, "nvnCommandBufferDrawArrays", &[7, 0xf001, 1, 3]);
        submit(&instance, &state, Status::Ok);
        call(&instance, "nvnQueuePresentTexture", &[0, 9, 0]);
        check_pixels(&state.frames.lock().unwrap().last().unwrap().2, 191);
        check_pixels(&state.memory.lock().unwrap()[0x2000..0x6000], 191);
        // Separate readback also proves the first value reached the output.
        begin();
        bind(red, 32);
        submit(&instance, &state, Status::Ok);
        call(&instance, "nvnQueuePresentTexture", &[0, 9, 0]);
        check_pixels(&state.frames.lock().unwrap().last().unwrap().2, 64);
        check_pixels(&state.memory.lock().unwrap()[0x2000..0x6000], 64);
        begin();
        bind(full, 65536);
        submit(&instance, &state, Status::Ok);
        check_pixels(&state.memory.lock().unwrap()[0x2000..0x6000], 191);
        for (offset, size) in [
            (red + 1, 32),
            (red, 0),
            (red, 31),
            (red, 65552),
            (0x30000 - 16, 32),
        ] {
            begin();
            bind(offset, size);
            submit(&instance, &state, Status::BadArgument);
        }
    }
    assert_eq!(
        translator.0.load(Ordering::Relaxed),
        2,
        "bindings do not translate again"
    );
    assert_eq!(
        instance.graphics_cache_stats().unwrap().misses,
        if properties.limits.max_uniform_buffer_range < 65536 {
            1
        } else {
            2
        },
        "one pipeline per selected buffer mode"
    );
    println!("MATCH stage-local bank 2, nonzero byte offsets, two bound colours, full 64 KiB range, uniform/storage readback and invalid ranges");
}

#[test]
#[ignore = "requires Vulkan, the flat arena, glslangValidator and spirv-val; never skips"]
fn uniform_banks_with_strip_and_normalized_attribute() {
    uniform_strip_proof(false);
}

#[test]
#[ignore = "requires Vulkan, the flat arena, glslangValidator and spirv-val; never skips"]
fn indexed_strip_uniform_depth_pixels() {
    uniform_strip_proof(true);
}

fn uniform_strip_proof(indexed_depth: bool) {
    use novena::gpu::uniforms::{UniformBankMapping, UniformBufferContract, UniformStage};
    let mut c = contract();
    c.topologies = vec![(0xf007, PrimitiveTopology::TriangleStrip)];
    c.attribute_formats.push((0xf106, VertexFormat::Unorm8x4));
    c.attribute_state_stride = Some(32);
    c.stream_state_stride = Some(16);
    if indexed_depth {
        c.depth_raster = depth_contract(false).depth_raster;
    }
    let f = Fixture::new(c);
    if indexed_depth {
        call(&f.instance, "nvnTextureBuilderSetDefaults", &[3]);
        call(&f.instance, "nvnTextureBuilderSetSize2D", &[3, 64, 64]);
        call(&f.instance, "nvnTextureBuilderSetFormat", &[3, 0xf006]);
        call(&f.instance, "nvnTextureBuilderSetTarget", &[3, 0xf005]);
        call(&f.instance, "nvnTextureBuilderSetStorage", &[3, 2, 0x20000]);
        call(&f.instance, "nvnTextureInitialize", &[5, 3]);
        call(
            &f.instance,
            "nvnDepthStencilStateSetDepthTestEnable",
            &[23, 1],
        );
        call(
            &f.instance,
            "nvnDepthStencilStateSetDepthWriteEnable",
            &[23, 1],
        );
        call(
            &f.instance,
            "nvnDepthStencilStateSetDepthFunc",
            &[23, 0xf101],
        );
        put(
            &f.state,
            0x1700,
            &[65535_u16, 2, 3, 4, 5, 65535]
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    f.program(
        Arc::new(SourceShaders {
            vertex: compile_source(
                "vert",
                r#"#version 450
layout(location = 0) in vec4 position;
layout(location = 1) in vec4 shade;
layout(location = 0) out vec4 tint;
layout(set = 0, binding = 2, std140) uniform Bank { vec4 data[4096]; } bank;
void main() { gl_Position = vec4(position.xyz, bank.data[0].x); tint = shade; }
"#,
            ),
            fragment: compile_source(
                "frag",
                r#"#version 450
layout(location = 0) in vec4 tint;
layout(location = 0) out vec4 color;
layout(set = 0, binding = 2, std140) uniform Bank { vec4 data[4096]; } bank;
void main() { color = bank.data[1] * tint; }
"#,
            ),
        }),
        &shader(true, 0.0),
        &shader(false, 0.0),
    );
    let vertices = [[8.0, 8.0], [56.0, 8.0], [8.0, 56.0], [56.0, 56.0]];
    put(&f.state, 0x1080, &[0x5a; 192]);
    for (i, &[x, y]) in vertices.iter().enumerate() {
        put(
            &f.state,
            0x1080 + (i + 1) * 32 + 8,
            &floats(&[x / 32.0 - 1.0, y / 32.0 - 1.0, 0.0, 1.0]),
        );
    }
    put(&f.state, 0x1800, &[0x5a, 255, 128, 64, 255]);
    call(&f.instance, "nvnVertexStreamStateSetDefaults", &[36]);
    call(&f.instance, "nvnVertexStreamStateSetStride", &[36, 0]);
    call(&f.instance, "nvnVertexStreamStateSetDivisor", &[36, 0]);
    call(&f.instance, "nvnVertexAttribStateSetDefaults", &[53]);
    call(
        &f.instance,
        "nvnVertexAttribStateSetFormat",
        &[53, 0xf106, 1],
    );
    call(&f.instance, "nvnVertexAttribStateSetStreamIndex", &[53, 1]);
    let context = novena::gpu::Context::new().expect("combined drawing proof");
    let limits = unsafe {
        context
            .instance
            .get_physical_device_properties(context.physical_device)
    }
    .limits;
    let alignment = limits
        .min_uniform_buffer_offset_alignment
        .max(limits.min_storage_buffer_offset_alignment)
        .max(16);
    let align = |offset: u64| offset.div_ceil(alignment) * alignment;
    let vertex = align(0x8000);
    let red = align(vertex + 0x1000);
    let green = align(red + 0x1000);
    assert!(green + 32 <= 0x30000);
    put(
        &f.state,
        (0x1000 + vertex) as usize,
        &floats(&[1.0, 0.0, 0.0, 0.0]),
    );
    for (offset, color) in [(red, [1.0, 0.25, 0.5, 1.0]), (green, [1.0, 0.75, 0.5, 1.0])] {
        put(&f.state, (0x1000 + offset + 16) as usize, &floats(&color));
    }
    let record = |offset, color_size| {
        if indexed_depth {
            put(&f.state, 0x21000, &1.0_f32.to_le_bytes().repeat(4096));
            put(&f.state, 0x25000, &[0; 4096]);
        }
        f.begin();
        if indexed_depth {
            call(
                &f.instance,
                "nvnCommandBufferSetRenderTargets",
                &[7, 1, 0x100, 0, 5, 0],
            );
        }
        call(
            &f.instance,
            "nvnCommandBufferBindVertexBuffer",
            &[7, 1, f.base + 0x800, color_size],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindVertexBuffer",
            &[7, 0, f.base + 0x80, 152],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindVertexStreamState",
            &[7, 2, 20],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindVertexAttribState",
            &[7, 2, 21],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindUniformBuffer",
            &[7, 0, 1, f.base + vertex, 16],
        );
        call(
            &f.instance,
            "nvnCommandBufferBindUniformBuffer",
            &[7, 1, 0, f.base + offset, 32],
        );
        if indexed_depth {
            call(
                &f.instance,
                "nvnCommandBufferDrawElementsBaseVertex",
                &[
                    7,
                    0xf007,
                    0xf006,
                    4,
                    f.base + 0x702,
                    u64::from((-1_i32) as u32),
                ],
            );
        } else {
            call(
                &f.instance,
                "nvnCommandBufferDrawArrays",
                &[7, 0xf007, 1, 4],
            );
        }
    };
    for storage in [false, true] {
        f.instance
            .set_uniform_buffer_contract(UniformBufferContract {
                bindings: vec![
                    UniformBankMapping {
                        stage: 0,
                        index: 1,
                        target: UniformStage::Vertex,
                        bank: 2,
                    },
                    UniformBankMapping {
                        stage: 1,
                        index: 0,
                        target: UniformStage::Fragment,
                        bank: 2,
                    },
                ],
                storage_buffers: storage,
            })
            .unwrap();
        record(red, 5);
        submit(&f.instance, &f.state, Status::Ok);
        wait_for_graphics(&f.instance);
        for (offset, color) in [(red, [255, 32, 32, 255]), (green, [255, 96, 32, 255])] {
            record(offset, 5);
            submit(&f.instance, &f.state, Status::Ok);
            check_geometry(&f.pixels(), &vertices, &[[0, 1, 2], [1, 2, 3]], color);
            if indexed_depth {
                let memory = f.state.memory.lock().unwrap();
                let center = 0x21000 + (24 * 64 + 32) * 4;
                assert_eq!(&memory[center..center + 4], &0.0_f32.to_le_bytes());
                assert_eq!(&memory[0x21000..0x21004], &1.0_f32.to_le_bytes());
                assert!(memory[0x25000..0x26000].iter().all(|&value| value == 0));
            }
        }
        record(red, 4);
        submit(&f.instance, &f.state, Status::BadArgument);
    }
    let misses = if limits.max_uniform_buffer_range < 65536 {
        1
    } else {
        2
    };
    assert_eq!(
        f.instance.graphics_cache_stats(),
        Some(CacheStats {
            misses,
            hits: 6 - misses
        })
    );
    println!("MATCH strip, normalized zero-stride second stream, stage-local banks, uniform/storage modes, rebinding, cache reuse and short vertex range; indexed depth {indexed_depth}");
}

fn depth_contract(clockwise: bool) -> FirstDrawContract {
    FirstDrawContract {
        topologies: vec![(0xf001, PrimitiveTopology::TriangleList)],
        attribute_formats: vec![(0xf002, VertexFormat::Float4)],
        attribute_state_stride: None,
        stream_state_stride: None,
        index_u16: 0xf006,
        index_u32: 0xf007,
        cull_none: 0xf003,
        rgba8: 0xf004,
        target_2d: 0xf005,
        identity_swizzle: [0; 4],
        depth_raster: Some(novena::gpu::graphics::DepthRasterContract {
            d32s8: 0xf006,
            compare: [
                0xf100, 0xf101, 0xf102, 0xf103, 0xf104, 0xf105, 0xf106, 0xf107,
            ],
            stencil_ops: [
                0xf200, 0xf201, 0xf202, 0xf203, 0xf204, 0xf205, 0xf206, 0xf207,
            ],
            faces: [0xf301, 0xf302, 0xf303],
            cull: [0xf401, 0xf402, 0xf403],
            polygon: [0xf500, 0xf501, 0xf502],
            clockwise,
            polygon_offset: true,
        }),
    }
}

struct DepthProof {
    instance: Instance,
    state: Box<State>,
    base: u64,
}
impl DepthProof {
    fn new() -> Self {
        let state = Box::new(State {
            memory: Mutex::new(vec![0; 0x20000]),
            frames: Mutex::new(Vec::new()),
        });
        let instance = unsafe {
            Instance::with_host(Host {
                user: std::ptr::from_ref(&*state).cast_mut().cast(),
                read_memory: Some(read),
                write_memory: Some(write),
                present: Some(present),
                ..Host::default()
            })
        };
        assert!(instance.set_first_draw_contract(Some(depth_contract(false))));
        call(&instance, "nvnMemoryPoolBuilderSetDefaults", &[1]);
        call(
            &instance,
            "nvnMemoryPoolBuilderSetStorage",
            &[1, 0x1000, 0xc000],
        );
        call(&instance, "nvnMemoryPoolInitialize", &[2, 1]);
        let base = call(&instance, "nvnMemoryPoolGetBufferAddress", &[2]);
        for (texture, format, offset) in [(4, 0xf004, 0x1000), (5, 0xf006, 0x6000)] {
            call(&instance, "nvnTextureBuilderSetDefaults", &[3]);
            call(&instance, "nvnTextureBuilderSetSize2D", &[3, 64, 64]);
            call(&instance, "nvnTextureBuilderSetFormat", &[3, format]);
            call(&instance, "nvnTextureBuilderSetTarget", &[3, 0xf005]);
            call(&instance, "nvnTextureBuilderSetStorage", &[3, 2, offset]);
            call(&instance, "nvnTextureInitialize", &[texture, 3]);
        }
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
            call(
                &instance,
                &format!(
                    "{}{kind}SetDefaults",
                    &functions::all().next().unwrap().1[..3]
                ),
                &[address],
            );
        }
        call(&instance, "nvnVertexStreamStateSetStride", &[20, 16]);
        call(&instance, "nvnVertexStreamStateSetDivisor", &[20, 0]);
        call(&instance, "nvnVertexAttribStateSetFormat", &[21, 0xf002, 0]);
        call(&instance, "nvnVertexAttribStateSetStreamIndex", &[21, 0]);
        call(&instance, "nvnColorStateSetBlendEnable", &[22, 0, 0]);
        for setting in ["DepthTestEnable", "DepthWriteEnable", "StencilTestEnable"] {
            call(
                &instance,
                &format!("nvnDepthStencilStateSet{setting}"),
                &[23, 0],
            );
        }
        call(&instance, "nvnDepthStencilStateSetDepthFunc", &[23, 0xf101]);
        call(&instance, "nvnPolygonStateSetCullFace", &[24, 0xf003]);
        call(&instance, "nvnPolygonStateSetPolygonMode", &[24, 0xf500]);
        instance.set_shader_translator(Some(Arc::new(Translator(AtomicUsize::new(0)))));
        instance.set_shader_translation_enabled(true);
        put(&state, 0x1500, &shader(false, 0.0));
        for (program, address, records, green) in
            [(12, 0x1300, 0x400, 0.25), (13, 0x1700, 0x480, 0.75)]
        {
            put(&state, address, &shader(true, green));
            put(
                &state,
                records,
                &(base + address as u64 - 0x1000).to_le_bytes(),
            );
            put(&state, records + 0x40, &(base + 0x500).to_le_bytes());
            call(&instance, "nvnProgramInitialize", &[program, 0]);
            call(
                &instance,
                "nvnProgramSetShaders",
                &[program, 2, records as u64],
            );
        }
        Self {
            instance,
            state,
            base,
        }
    }
    fn reset(&self, depth: f32, stencil: u8) {
        put(&self.state, 0x2000, &[0, 0, 255, 255].repeat(4096));
        put(&self.state, 0x7000, &depth.to_le_bytes().repeat(4096));
        put(&self.state, 0xb000, &[stencil; 4096]);
    }
    fn vertices(&self, z: f32, reverse: bool) {
        let mut vertices = [
            [-0.75, -0.75, z, 1.0],
            [0.75, -0.75, z, 1.0],
            [0.0, 0.75, z, 1.0],
        ];
        if reverse {
            vertices.swap(1, 2);
        }
        put(
            &self.state,
            0x1080,
            &vertices
                .into_iter()
                .flatten()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    fn record(&self, program: u64, stencil: [u64; 3], scissor: u64, bias: Option<[f32; 3]>) {
        let i = &self.instance;
        call(i, "nvnCommandBufferBeginRecording", &[7]);
        call(
            i,
            "nvnCommandBufferSetRenderTargets",
            &[7, 1, 0x100, 0, 5, 0],
        );
        call(i, "nvnCommandBufferBindProgram", &[7, program, 0xffff]);
        call(
            i,
            "nvnCommandBufferBindVertexBuffer",
            &[7, 0, self.base + 0x80, 48],
        );
        call(i, "nvnCommandBufferBindVertexStreamState", &[7, 1, 20]);
        call(i, "nvnCommandBufferBindVertexAttribState", &[7, 1, 21]);
        call(i, "nvnCommandBufferBindColorState", &[7, 22]);
        call(i, "nvnCommandBufferBindDepthStencilState", &[7, 23]);
        call(i, "nvnCommandBufferBindPolygonState", &[7, 24]);
        call(
            i,
            "nvnCommandBufferSetStencilMask",
            &[7, 0xf303, stencil[0]],
        );
        call(i, "nvnCommandBufferSetStencilRef", &[7, 0xf303, stencil[1]]);
        call(
            i,
            "nvnCommandBufferSetStencilValueMask",
            &[7, 0xf303, stencil[2]],
        );
        call(i, "nvnCommandBufferSetViewport", &[7, 0, 0, 64, 64]);
        call(i, "nvnCommandBufferSetScissor", &[7, 0, 0, scissor, 64]);
        if let Some(bias) = bias {
            let mut r = Registers::default();
            r.x[0] = 7;
            for (j, value) in bias.into_iter().enumerate() {
                r.d[j] = u64::from(value.to_bits());
            }
            assert_eq!(
                i.call(
                    functions::lookup("nvnCommandBufferSetPolygonOffsetClamp").unwrap(),
                    &mut r
                ),
                Status::Ok
            );
        }
        call(i, "nvnCommandBufferDrawArrays", &[7, 0xf001, 0, 3]);
    }
    // A warmup can execute. Restore arena bytes before the measured draw.
    fn warm(&self, program: u64, mask: u64, reference: u64, scissor: u64, bias: Option<[f32; 3]>) {
        self.record(program, [mask, reference, 0xff], scissor, bias);
        submit(&self.instance, &self.state, Status::Ok);
        wait_for_graphics(&self.instance);
    }
    fn draw(&self, program: u64, mask: u64, reference: u64, scissor: u64, bias: Option<[f32; 3]>) {
        self.record(program, [mask, reference, 0xff], scissor, bias);
        submit(&self.instance, &self.state, Status::Ok);
    }
    fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        call(&self.instance, "nvnQueuePresentTexture", &[0, 9, 0]);
        let frames = self.state.frames.lock().unwrap();
        let p = &frames.last().unwrap().2[(y * 64 + x) * 4..(y * 64 + x + 1) * 4];
        let bytes = self.state.memory.lock().unwrap();
        assert_eq!(
            p,
            &bytes[0x2000 + (y * 64 + x) * 4..0x2000 + (y * 64 + x + 1) * 4]
        );
        p.try_into().unwrap()
    }
    fn depth(&self) -> f32 {
        let memory = self.state.memory.lock().unwrap();
        let at = 0x7000 + (24 * 64 + 32) * 4;
        f32::from_le_bytes(memory[at..at + 4].try_into().unwrap())
    }
    fn stencil(&self, x: usize) -> u8 {
        self.state.memory.lock().unwrap()[0xb000 + 24 * 64 + x]
    }
    fn setting(&self, name: &str, value: u64) {
        call(
            &self.instance,
            &format!("nvnDepthStencilStateSet{name}"),
            &[23, value],
        );
    }
    fn stencil_state(&self, compare: u64, ops: [u64; 3]) {
        call(
            &self.instance,
            "nvnDepthStencilStateSetStencilFunc",
            &[23, 0xf303, compare, 0, 0xff],
        );
        call(
            &self.instance,
            "nvnDepthStencilStateSetStencilOp",
            &[23, 0xf303, ops[0], ops[1], ops[2]],
        );
    }
}

#[test]
#[ignore = "requires Vulkan, the flat arena, spirv-val and Shadowbox; never skips"]
fn depth_stencil_and_raster_pixels() {
    let p = DepthProof::new();
    let first = [255, 64, 128, 255];
    let second = [255, 191, 128, 255];
    let clear = [0, 0, 255, 255];
    p.vertices(0.25, false);
    p.reset(1.0, 0);
    p.setting("DepthTestEnable", 1);
    p.setting("DepthWriteEnable", 1);
    p.warm(12, 0xff, 0, 64, None);
    p.warm(13, 0xff, 0, 64, None);
    p.reset(1.0, 0);
    p.draw(12, 0xff, 0, 64, None);
    assert_eq!(p.depth(), 0.25);
    p.vertices(0.75, false);
    p.draw(13, 0xff, 0, 64, None);
    assert_eq!(p.pixel(32, 24), first);
    assert_eq!(p.depth(), 0.25);
    // Reversing draw order still leaves the nearer fragment.
    p.reset(1.0, 0);
    p.draw(13, 0xff, 0, 64, None);
    p.vertices(0.25, false);
    p.draw(12, 0xff, 0, 64, None);
    assert_eq!(p.pixel(32, 24), first);
    // Disabled writes preserve the initial depth and allow the farther draw.
    p.setting("DepthWriteEnable", 0);
    p.warm(12, 0xff, 0, 64, None);
    p.warm(13, 0xff, 0, 64, None);
    p.reset(1.0, 0);
    p.draw(12, 0xff, 0, 64, None);
    p.vertices(0.75, false);
    p.draw(13, 0xff, 0, 64, None);
    assert_eq!(p.pixel(32, 24), second);
    assert_eq!(p.depth(), 1.0);
    // Depth write enable has no effect when the depth test is disabled.
    p.setting("DepthTestEnable", 0);
    p.setting("DepthWriteEnable", 1);
    p.vertices(0.25, false);
    p.warm(12, 0xff, 0, 64, None);
    p.reset(0.5, 0);
    p.draw(12, 0xff, 0, 64, None);
    assert_eq!(p.pixel(32, 24), first);
    assert_eq!(p.depth(), 0.5);
    p.setting("DepthTestEnable", 1);
    p.setting("DepthWriteEnable", 0);
    // Every compare token is exercised on equal and unequal depths.
    for (op, pass) in [
        (0, false),
        (1, false),
        (2, true),
        (3, true),
        (4, false),
        (5, false),
        (6, true),
        (7, true),
    ] {
        p.setting("DepthFunc", 0xf100 + op);
        p.vertices(0.5, false);
        p.warm(12, 0xff, 0, 64, None);
        p.reset(0.5, 0);
        p.draw(12, 0xff, 0, 64, None);
        assert_eq!(p.pixel(32, 24), if pass { first } else { clear });
        p.reset(0.75, 0);
        p.draw(12, 0xff, 0, 64, None);
        assert_eq!(
            p.pixel(32, 24),
            if matches!(op, 1 | 3 | 5 | 7) {
                first
            } else {
                clear
            }
        );
    }
    p.setting("DepthTestEnable", 0);
    p.setting("StencilTestEnable", 1);
    p.stencil_state(0xf107, [0xf200, 0xf200, 0xf202]);
    p.warm(12, 0x0f, 0xa5, 32, None);
    p.reset(1.0, 0xb0);
    p.draw(12, 0x0f, 0xa5, 32, None);
    assert_eq!(p.stencil(24), 0xb5);
    assert_eq!(p.stencil(40), 0xb0);
    // Compare against the mask through a second draw, with writes disabled.
    p.stencil_state(0xf102, [0xf200; 3]);
    p.warm(13, 0, 0xb5, 64, None);
    // Warmup may color the left side. Rebuild the mask before measuring.
    p.stencil_state(0xf107, [0xf200, 0xf200, 0xf202]);
    p.reset(1.0, 0xb0);
    p.draw(12, 0x0f, 0xa5, 32, None);
    p.stencil_state(0xf102, [0xf200; 3]);
    p.draw(13, 0, 0xb5, 64, None);
    assert_eq!(p.pixel(24, 24), second);
    assert_eq!(p.pixel(40, 24), clear);
    assert_eq!(p.stencil(24), 0xb5);
    assert_eq!(p.stencil(40), 0xb0);
    // Comparison masks ignore the high reference and stored bits independently of writes.
    p.record(13, [0, 5, 15], 64, None);
    submit(&p.instance, &p.state, Status::Ok);
    wait_for_graphics(&p.instance);
    p.reset(1.0, 0xa5);
    p.record(13, [0, 5, 15], 64, None);
    submit(&p.instance, &p.state, Status::Ok);
    assert_eq!(p.pixel(32, 24), second);
    assert_eq!(p.stencil(32), 0xa5);
    p.warm(13, 0, 5, 64, None);
    p.reset(1.0, 0xa5);
    p.draw(13, 0, 5, 64, None);
    assert_eq!(p.pixel(32, 24), clear);
    // Dynamic references distinguish front and back when culling is disabled.
    p.stencil_state(0xf107, [0xf200, 0xf200, 0xf202]);
    for reverse in [false, true] {
        p.vertices(0.5, reverse);
        for measured in [false, true] {
            p.reset(1.0, 0);
            p.record(12, [0xff, 0, 0xff], 64, None);
            call(
                &p.instance,
                "nvnCommandBufferSetStencilRef",
                &[7, 0xf301, 5],
            );
            call(
                &p.instance,
                "nvnCommandBufferSetStencilRef",
                &[7, 0xf302, 6],
            );
            call(
                &p.instance,
                "nvnCommandBufferDrawArrays",
                &[7, 0xf001, 0, 3],
            );
            submit(&p.instance, &p.state, Status::Ok);
            wait_for_graphics(&p.instance);
            if measured {
                assert_eq!(p.stencil(32), if reverse { 5 } else { 6 });
            }
        }
    }
    p.vertices(0.5, false);
    // All operation mappings, including clamping and wrapping.
    for (op, initial, reference, expected) in [
        (0, 9, 3, 9),
        (1, 9, 3, 0),
        (2, 9, 3, 3),
        (3, 255, 0, 255),
        (4, 0, 0, 0),
        (5, 0x55, 0, 0xaa),
        (6, 255, 0, 0),
        (7, 0, 0, 255),
    ] {
        p.stencil_state(0xf107, [0xf200, 0xf200, 0xf200 + op]);
        p.warm(12, 0xff, reference, 64, None);
        p.reset(1.0, initial);
        p.draw(12, 0xff, reference, 64, None);
        assert_eq!(p.stencil(32), expected);
    }
    // Distinguish stencil fail, depth fail and pass positions.
    p.stencil_state(0xf100, [0xf202, 0xf201, 0xf200]);
    p.warm(12, 0xff, 7, 64, None);
    p.reset(1.0, 9);
    p.draw(12, 0xff, 7, 64, None);
    assert_eq!(p.stencil(32), 7);
    assert_eq!(p.pixel(32, 24), clear);
    p.setting("DepthTestEnable", 1);
    p.setting("DepthFunc", 0xf100);
    p.stencil_state(0xf107, [0xf200, 0xf202, 0xf201]);
    p.warm(12, 0xff, 6, 64, None);
    p.reset(1.0, 9);
    p.draw(12, 0xff, 6, 64, None);
    assert_eq!(p.stencil(32), 6);
    assert_eq!(p.pixel(32, 24), clear);
    p.setting("StencilTestEnable", 0);
    p.setting("DepthTestEnable", 0);
    for clockwise in [false, true] {
        assert!(p
            .instance
            .set_first_draw_contract(Some(depth_contract(clockwise))));
        for cull in [0xf003, 0xf401, 0xf402, 0xf403] {
            call(&p.instance, "nvnPolygonStateSetCullFace", &[24, cull]);
            for reverse in [false, true] {
                p.vertices(0.5, reverse);
                p.warm(12, 0, 0, 64, None);
                p.reset(1.0, 0);
                p.draw(12, 0, 0, 64, None);
                // Positive viewport height: the original order is clockwise in framebuffer coordinates.
                let front = clockwise != reverse;
                let visible =
                    cull == 0xf003 || (cull == 0xf401 && !front) || (cull == 0xf402 && front);
                assert_eq!(
                    p.pixel(32, 24),
                    if visible { first } else { clear },
                    "cull={cull:x} clockwise={clockwise} reverse={reverse}"
                );
            }
        }
    }
    call(&p.instance, "nvnPolygonStateSetCullFace", &[24, 0xf003]);
    for mode in [0xf501, 0xf502] {
        call(&p.instance, "nvnPolygonStateSetPolygonMode", &[24, mode]);
        p.warm(12, 0, 0, 64, None);
        p.reset(1.0, 0);
        p.draw(12, 0, 0, 64, None);
        assert_eq!(p.pixel(32, 24), clear);
        let memory = p.state.memory.lock().unwrap();
        assert!(
            memory[0x2000..0x6000].as_chunks::<4>().0.contains(&first),
            "non-solid mode must rasterize"
        );
    }
    call(&p.instance, "nvnPolygonStateSetPolygonMode", &[24, 0xf500]);
    // A positive constant bias rejects equal-depth fragments under LESS_OR_EQUAL.
    p.setting("DepthTestEnable", 1);
    p.setting("DepthFunc", 0xf103);
    p.vertices(0.5, false);
    p.warm(12, 0, 0, 64, None);
    p.warm(13, 0, 0, 64, Some([0.0, 1_000_000.0, 0.0]));
    p.reset(0.5, 0);
    p.draw(12, 0, 0, 64, None);
    p.draw(13, 0, 0, 64, Some([0.0, 1_000_000.0, 0.0]));
    assert_eq!(p.pixel(32, 24), first);
    // Slope bias changes the depth plane; clamp bounds that change.
    p.setting("DepthWriteEnable", 1);
    p.setting("DepthFunc", 0xf107);
    let vertices = [
        [-0.75_f32, -0.75, 0.25, 1.0],
        [0.75, -0.75, 0.75, 1.0],
        [0.0, 0.75, 0.5, 1.0],
    ];
    put(
        &p.state,
        0x1080,
        &vertices
            .into_iter()
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    for bias in [None, Some([12.0, 0.0, 0.0]), Some([12.0, 0.0, 0.02])] {
        p.warm(12, 0, 0, 64, bias);
    }
    p.reset(1.0, 0);
    p.draw(12, 0, 0, 64, None);
    let baseline = p.depth();
    p.reset(1.0, 0);
    p.draw(12, 0, 0, 64, Some([12.0, 0.0, 0.0]));
    let slope = p.depth();
    p.reset(1.0, 0);
    p.draw(12, 0, 0, 64, Some([12.0, 0.0, 0.02]));
    let clamped = p.depth();
    assert!(
        (slope - baseline - 0.125).abs() < 0.001,
        "slope delta {}",
        slope - baseline
    );
    assert!(
        (clamped - baseline - 0.02).abs() < 0.001,
        "clamp delta {}",
        clamped - baseline
    );
    assert_eq!(p.pixel(32, 24), first);
    println!("MATCH depth ordering, depth write mask, compares, stencil mask and operations, both windings, front face, polygon modes and depth bias through recorded state, arena and pixel readback");
}

fn texture_contract(set: u32) -> novena::gpu::textures::TextureContract {
    use ash::vk;
    use novena::gpu::{textures::TextureMapping, uniforms::UniformStage};
    novena::gpu::textures::TextureContract {
        enums: None,
        bindings: vec![TextureMapping {
            stage: 5,
            index: 3,
            target: UniformStage::Fragment,
            set,
            image: 0,
            sampler: 1,
        }],
        filters: vec![(100, vk::Filter::NEAREST), (101, vk::Filter::LINEAR)],
        wraps: vec![
            (200, vk::SamplerAddressMode::REPEAT),
            (201, vk::SamplerAddressMode::MIRRORED_REPEAT),
            (202, vk::SamplerAddressMode::CLAMP_TO_EDGE),
            (203, vk::SamplerAddressMode::CLAMP_TO_BORDER),
        ],
        compare_disabled: 300,
        lod: None,
        combined: vec![(0x1234_5678, 256, 257)],
    }
}

fn blend_contract() -> novena::gpu::graphics::BlendContract {
    use ash::vk;
    novena::gpu::graphics::BlendContract {
        factors: vec![
            (400, vk::BlendFactor::ZERO),
            (401, vk::BlendFactor::ONE),
            (402, vk::BlendFactor::SRC_ALPHA),
            (403, vk::BlendFactor::ONE_MINUS_SRC_ALPHA),
        ],
        operations: vec![
            (500, vk::BlendOp::ADD),
            (501, vk::BlendOp::SUBTRACT),
            (502, vk::BlendOp::REVERSE_SUBTRACT),
            (503, vk::BlendOp::MIN),
            (504, vk::BlendOp::MAX),
        ],
        function_order: [0, 1, 2, 3],
        equation_order: [0, 1],
        channel_order: [0, 1, 2, 3],
    }
}

fn texture_resources(f: &Fixture) {
    let i = &f.instance;
    call(i, "nvnTextureBuilderSetDefaults", &[30]);
    call(i, "nvnTextureBuilderSetSize2D", &[30, 2, 2]);
    call(i, "nvnTextureBuilderSetFormat", &[30, 0xf004]);
    call(i, "nvnTextureBuilderSetTarget", &[30, 0xf005]);
    call(i, "nvnTextureBuilderSetStorage", &[30, 2, 0x6000]);
    call(i, "nvnTextureInitialize", &[31, 30]);
    // Two white and two black texels. Every texel has half alpha.
    put(
        &f.state,
        0x7000,
        &[
            255, 255, 255, 128, 0, 0, 0, 128, 0, 0, 0, 128, 255, 255, 255, 128,
        ],
    );
    call(i, "nvnTexturePoolInitialize", &[32, 2, 0, 512]);
    call(i, "nvnSamplerPoolInitialize", &[33, 2, 0, 512]);
    call(i, "nvnTexturePoolRegisterTexture", &[32, 256, 31, 0]);
    assert_eq!(call(i, "nvnDeviceGetSeparateTextureHandle", &[0, 256]), 256);
    call(i, "nvnSamplerBuilderSetDefaults", &[34]);
    call(i, "nvnSamplerBuilderSetCompare", &[34, 300, 0]);
    let mut r = Registers::default();
    r.x[0] = 34;
    r.d[0] = u64::from(1.0_f32.to_bits());
    assert_eq!(
        i.call(
            functions::lookup("nvnSamplerBuilderSetMaxAnisotropy").unwrap(),
            &mut r
        ),
        Status::Ok
    );
}

fn texture_sampler(f: &Fixture, filter: u64, wrap: u64) {
    call(
        &f.instance,
        "nvnSamplerBuilderSetMinMagFilter",
        &[34, filter, filter],
    );
    call(
        &f.instance,
        "nvnSamplerBuilderSetWrapMode",
        &[34, wrap, wrap, wrap],
    );
    call(&f.instance, "nvnSamplerInitialize", &[35, 34]);
    call(&f.instance, "nvnSamplerPoolRegisterSampler", &[33, 257, 35]);
    assert_eq!(
        call(&f.instance, "nvnDeviceGetSeparateSamplerHandle", &[0, 257]),
        257
    );
}

fn texture_draw(f: &Fixture, combined: bool, blend: bool, mask: bool) {
    texture_draw_state(f, combined, blend, mask);
    call(
        &f.instance,
        "nvnCommandBufferDrawArrays",
        &[7, 0xf001, 1, 3],
    );
}

fn texture_draw_state(f: &Fixture, combined: bool, blend: bool, mask: bool) {
    f.begin();
    let i = &f.instance;
    call(
        i,
        "nvnCommandBufferBindVertexBuffer",
        &[7, 0, f.base + 0x80, 128],
    );
    call(i, "nvnCommandBufferBindVertexStreamState", &[7, 1, 20]);
    call(i, "nvnCommandBufferBindVertexAttribState", &[7, 1, 21]);
    call(i, "nvnCommandBufferSetTexturePool", &[7, 32]);
    call(i, "nvnCommandBufferSetSamplerPool", &[7, 33]);
    if combined {
        call(i, "nvnCommandBufferBindTexture", &[7, 5, 3, 0x1234_5678]);
    } else {
        call(i, "nvnCommandBufferBindSeparateTexture", &[7, 5, 3, 256]);
        call(i, "nvnCommandBufferBindSeparateSampler", &[7, 5, 3, 257]);
    }
    if blend {
        call(i, "nvnCommandBufferBindBlendState", &[7, 36]);
    }
    if mask {
        call(i, "nvnCommandBufferBindChannelMaskState", &[7, 37]);
    }
}

fn texture_vertices(f: &Fixture) {
    // One oversized triangle covers every pixel, with no diagonal seam.
    for (n, p) in [
        [-1.0, -1.0, 0.0, 1.0],
        [3.0, -1.0, 0.0, 1.0],
        [-1.0, 3.0, 0.0, 1.0],
    ]
    .iter()
    .enumerate()
    {
        put(&f.state, 0x1080 + (n + 1) * 32 + 8, &floats(p));
    }
}

fn checker_reference(u: f32, v: f32, linear: bool, wrap: u64) -> [u8; 4] {
    let texel = |x: i32, y: i32| -> [f32; 4] {
        let coord = |p: i32| -> Option<i32> {
            match wrap {
                200 => Some(p.rem_euclid(2)),
                201 => {
                    let p = p.rem_euclid(4);
                    Some(if p < 2 { p } else { 3 - p })
                }
                202 => Some(p.clamp(0, 1)),
                203 => (0..2).contains(&p).then_some(p),
                _ => unreachable!(),
            }
        };
        match (coord(x), coord(y)) {
            (Some(x), Some(y)) => {
                let c = if x == y { 255.0 } else { 0.0 };
                [c, c, c, 128.0]
            }
            _ => [0.0; 4],
        }
    };
    if !linear {
        return texel((u * 2.0).floor() as i32, (v * 2.0).floor() as i32).map(|v| v as u8);
    }
    let x = u * 2.0 - 0.5;
    let y = v * 2.0 - 0.5;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let mut color = [0.0; 4];
    for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
        for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
            let p = texel(ix + dx, iy + dy);
            for c in 0..4 {
                color[c] += p[c] * wx * wy;
            }
        }
    }
    color.map(|v| v.round() as u8)
}

#[test]
#[ignore = "requires Vulkan, glslangValidator, spirv-val and Shadowbox; never skips"]
fn textured_checkerboard_and_blend_pixels() {
    let f = Fixture::new(contract());
    texture_resources(&f);
    texture_vertices(&f);
    f.instance
        .set_texture_contract(texture_contract(1))
        .unwrap();
    f.instance
        .set_blend_contract(Some(blend_contract()))
        .unwrap();
    let vertex = compile_source(
        "vert",
        "#version 450\nlayout(location=0) in vec4 p; void main(){ gl_Position=p; }",
    );
    let fragment=compile_source("frag", "#version 450\nlayout(set=1,binding=0) uniform texture2D t; layout(set=1,binding=1) uniform sampler s; layout(location=0) out vec4 c; void main(){ c=texture(sampler2D(t,s), gl_FragCoord.xy/32.0-vec2(0.5)); }");
    f.program(
        Arc::new(SourceShaders { vertex, fragment }),
        &shader(true, 0.0),
        &shader(false, 0.0),
    );
    texture_sampler(&f, 100, 200);
    texture_draw(&f, false, false, false);
    submit(&f.instance, &f.state, Status::Ok);
    wait_for_graphics(&f.instance);
    for filter in [100, 101] {
        for wrap in [200, 201, 202, 203] {
            for combined in [false, true] {
                texture_sampler(&f, filter, wrap);
                texture_draw(&f, combined, false, false);
                submit(&f.instance, &f.state, Status::Ok);
                let pixels = f.pixels();
                for y in 0..64 {
                    for x in 0..64 {
                        let expected = checker_reference(
                            (x as f32 + 0.5) / 32.0 - 0.5,
                            (y as f32 + 0.5) / 32.0 - 0.5,
                            filter == 101,
                            wrap,
                        );
                        let actual = &pixels[(y * 64 + x) * 4..(y * 64 + x + 1) * 4];
                        for c in 0..4 {
                            assert!(actual[c].abs_diff(expected[c])<=2, "filter {filter} wrap {wrap} combined {combined} at {x},{y} channel {c}: {} != {}",actual[c],expected[c]);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(f.instance.graphics_cache_stats().unwrap().misses, 1);
    call(&f.instance, "nvnBlendStateSetDefaults", &[36]);
    call(&f.instance, "nvnBlendStateSetBlendTarget", &[36, 0]);
    call(
        &f.instance,
        "nvnBlendStateSetBlendFunc",
        &[36, 402, 403, 401, 400],
    );
    call(
        &f.instance,
        "nvnBlendStateSetBlendEquation",
        &[36, 500, 500],
    );
    call(&f.instance, "nvnColorStateSetBlendEnable", &[22, 0, 1]);
    call(&f.instance, "nvnChannelMaskStateSetDefaults", &[37]);
    call(
        &f.instance,
        "nvnChannelMaskStateSetChannelMask",
        &[37, 0, 1, 0, 1, 0],
    );
    texture_sampler(&f, 100, 200);
    for mask in [false, true] {
        texture_draw(&f, false, true, mask);
        submit(&f.instance, &f.state, Status::Ok);
        wait_for_graphics(&f.instance);
        texture_draw(&f, false, true, mask);
        submit(&f.instance, &f.state, Status::Ok);
        let pixels = f.pixels();
        for y in 0..64 {
            for x in 0..64 {
                let p = checker_reference(
                    (x as f32 + 0.5) / 32.0 - 0.5,
                    (y as f32 + 0.5) / 32.0 - 0.5,
                    false,
                    200,
                );
                let white = p[0] != 0;
                let expected = if mask {
                    if white {
                        [128, 0, 255, 255]
                    } else {
                        [0, 0, 127, 255]
                    }
                } else if white {
                    [128, 128, 255, 128]
                } else {
                    [0, 0, 127, 128]
                };
                for c in 0..4 {
                    assert!(
                        pixels[(y * 64 + x) * 4 + c].abs_diff(expected[c]) <= 1,
                        "blend mask {mask} at {x},{y}"
                    );
                }
            }
        }
    }
    assert_eq!(f.instance.graphics_cache_stats().unwrap().misses, 3);
    // A registered view, unknown handle and compare sampling fail closed on a ready pipeline.
    call(
        &f.instance,
        "nvnTexturePoolRegisterTexture",
        &[32, 256, 31, 999],
    );
    texture_draw(&f, false, true, false);
    submit(&f.instance, &f.state, Status::Unimplemented);
    call(
        &f.instance,
        "nvnTexturePoolRegisterTexture",
        &[32, 256, 31, 0],
    );
    call(&f.instance, "nvnSamplerBuilderSetCompare", &[34, 999, 0]);
    texture_sampler(&f, 100, 200);
    texture_draw(&f, false, true, false);
    submit(&f.instance, &f.state, Status::Unimplemented);
    println!("MATCH all checkerboard pixels: nearest, linear, four wraps, separate and combined references, alpha blending, channel masks and cache reuse");
}

#[test]
#[ignore = "requires Vulkan, spirv-val and Shadowbox; never skips"]
fn translated_texture_pixels() {
    let f = Fixture::new(contract());
    texture_resources(&f);
    texture_vertices(&f);
    texture_sampler(&f, 100, 200);
    f.instance
        .set_texture_contract(texture_contract(0))
        .unwrap();
    // envytools gm107: TEX.B.LZ, R0 output, R4 coordinates, R6 constant handle,
    // 2D bit 29, all four components in bits 31..34, level-zero bit 37.
    let mut header = [0_u32; 20];
    header[0] = 2 | (3 << 5) | (5 << 10) | (1 << 14);
    header[18] = 15;
    let fragment = pack_shader(
        header,
        vec![
            mov(4, 0.25),
            mov(5, 0.25),
            mov(6, f32::from_bits(7)),
            0xdeb8_0000_0000_0000
                | ALWAYS
                | (4 << 8)
                | (6 << 20)
                | (1 << 29)
                | (15 << 31)
                | (1 << 37),
        ],
    );
    f.program(
        Arc::new(Translator(AtomicUsize::new(0))),
        &fragment,
        &shader(false, 0.0),
    );
    texture_draw(&f, false, false, false);
    submit(&f.instance, &f.state, Status::Ok);
    wait_for_graphics(&f.instance);
    texture_draw(&f, false, false, false);
    submit(&f.instance, &f.state, Status::Ok);
    assert!(f
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 255, 255, 128]));
    // Re-register the same handle with a different image. The cached image view must follow it.
    call(&f.instance, "nvnTextureBuilderSetStorage", &[30, 2, 0x6100]);
    call(&f.instance, "nvnTextureInitialize", &[38, 30]);
    put(&f.state, 0x7100, &[10, 20, 30, 40].repeat(4));
    call(
        &f.instance,
        "nvnTexturePoolRegisterTexture",
        &[32, 256, 38, 0],
    );
    texture_draw(&f, false, false, false);
    submit(&f.instance, &f.state, Status::Ok);
    assert!(f
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [10, 20, 30, 40]));
    assert_eq!(f.instance.graphics_cache_stats().unwrap().misses, 1);
    call(&f.instance, "nvnTextureBuilderSetSize2D", &[30, 1, 1]);
    call(&f.instance, "nvnTextureInitialize", &[38, 30]);
    texture_draw(&f, false, false, false);
    submit(&f.instance, &f.state, Status::Ok);
    assert!(f
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [10, 20, 30, 40]));
    // Each separate setter overrides its component of a combined reference.
    call(&f.instance, "nvnSamplerPoolRegisterSampler", &[33, 258, 35]);
    texture_draw(&f, true, false, false);
    call(
        &f.instance,
        "nvnCommandBufferBindSeparateSampler",
        &[7, 5, 3, 258],
    );
    call(
        &f.instance,
        "nvnCommandBufferDrawArrays",
        &[7, 0xf001, 1, 3],
    );
    submit(&f.instance, &f.state, Status::Ok);
    assert!(f
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [10, 20, 30, 40]));
    call(
        &f.instance,
        "nvnTexturePoolRegisterTexture",
        &[32, 258, 31, 0],
    );
    texture_draw(&f, true, false, false);
    call(
        &f.instance,
        "nvnCommandBufferBindSeparateTexture",
        &[7, 5, 3, 258],
    );
    call(
        &f.instance,
        "nvnCommandBufferDrawArrays",
        &[7, 0xf001, 1, 3],
    );
    submit(&f.instance, &f.state, Status::Ok);
    assert!(f
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 255, 255, 128]));
    println!("MATCH translated separate texture/sampler descriptors, constant handle source and re-registration");
}

#[test]
#[ignore = "requires Vulkan, glslangValidator and spirv-val; never skips"]
fn multiple_target_blend_pixels() {
    let f = Fixture::new(contract());
    texture_vertices(&f);
    f.instance
        .set_blend_contract(Some(blend_contract()))
        .unwrap();
    call(&f.instance, "nvnTextureBuilderSetStorage", &[3, 2, 0x8000]);
    call(&f.instance, "nvnTextureInitialize", &[40, 3]);
    put(
        &f.state,
        0x100,
        &[4_u64, 40]
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    f.program(Arc::new(SourceShaders {
        vertex:compile_source("vert","#version 450\nlayout(location=0) in vec4 p; void main(){gl_Position=p;}"),
        fragment:compile_source("frag","#version 450\nlayout(location=0) out vec4 a; layout(location=1) out vec4 b; void main(){a=vec4(1,0,0,0.5); b=vec4(0,1,0,0.25);}"),
    }),&shader(true,0.0),&shader(false,0.0));
    for target in [0, 1] {
        call(&f.instance, "nvnColorStateSetBlendEnable", &[22, target, 1]);
        call(&f.instance, "nvnBlendStateSetDefaults", &[36 + target]);
        call(
            &f.instance,
            "nvnBlendStateSetBlendTarget",
            &[36 + target, target],
        );
        call(
            &f.instance,
            "nvnBlendStateSetBlendFunc",
            if target == 0 {
                &[36, 402, 403, 401, 400]
            } else {
                &[37, 401, 400, 400, 401]
            },
        );
        call(
            &f.instance,
            "nvnBlendStateSetBlendEquation",
            &[36 + target, 500, 500],
        );
    }
    call(&f.instance, "nvnChannelMaskStateSetDefaults", &[38]);
    call(
        &f.instance,
        "nvnChannelMaskStateSetChannelMask",
        &[38, 0, 1, 1, 1, 1],
    );
    call(
        &f.instance,
        "nvnChannelMaskStateSetChannelMask",
        &[38, 1, 0, 1, 1, 0],
    );
    let draw = || {
        triangle_state(&f.instance, &f.state, f.base + 0x80, 128);
        call(
            &f.instance,
            "nvnCommandBufferSetRenderTargets",
            &[7, 2, 0x100, 0, 0, 0],
        );
        put(&f.state, 0x200, &floats(&[0.0, 0.0, 1.0, 1.0]));
        put(&f.state, 0x220, &floats(&[1.0, 0.0, 0.0, 1.0]));
        call(
            &f.instance,
            "nvnCommandBufferClearColor",
            &[7, 0, 0x200, 15],
        );
        call(
            &f.instance,
            "nvnCommandBufferClearColor",
            &[7, 1, 0x220, 15],
        );
        // Reverse binding order catches accidental global blend replacement.
        call(&f.instance, "nvnCommandBufferBindBlendState", &[7, 37]);
        call(&f.instance, "nvnCommandBufferBindBlendState", &[7, 36]);
        call(
            &f.instance,
            "nvnCommandBufferBindChannelMaskState",
            &[7, 38],
        );
        call(
            &f.instance,
            "nvnCommandBufferDrawArrays",
            &[7, 0xf001, 1, 3],
        );
    };
    draw();
    submit(&f.instance, &f.state, Status::Ok);
    wait_for_graphics(&f.instance);
    draw();
    submit(&f.instance, &f.state, Status::Ok);
    assert!(f
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| p[0].abs_diff(128) <= 1
            && p[1] == 0
            && p[2].abs_diff(127) <= 1
            && p[3].abs_diff(128) <= 1));
    assert!(f.state.memory.lock().unwrap()[0x9000..0xd000]
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 255, 0, 255]));
    call(
        &f.instance,
        "nvnChannelMaskStateSetChannelMask",
        &[38, 1, 1, 1, 1, 1],
    );
    draw();
    submit(&f.instance, &f.state, Status::Ok);
    wait_for_graphics(&f.instance);
    draw();
    submit(&f.instance, &f.state, Status::Ok);
    assert!(f.state.memory.lock().unwrap()[0x9000..0xd000]
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [0, 255, 0, 255]));
    assert_eq!(f.instance.graphics_cache_stats().unwrap().misses, 2);
    println!("MATCH independent blend equations, alpha factors, write masks and cache keys for two render targets");
}

#[test]
#[ignore = "requires Vulkan, glslangValidator and spirv-val; never skips"]
fn textured_uniform_banks_and_persistence() {
    use novena::gpu::uniforms::{UniformBankMapping, UniformBufferContract, UniformStage};
    let cache = std::env::temp_dir().join(format!("textured-cache-{}", std::process::id()));
    if cache.exists() {
        fs::remove_dir_all(&cache).unwrap();
    }
    let identity = novena::gpu::pipelines::TranslationIdentity {
        version: "original-textured-uniform-1".into(),
        configuration: "four stage descriptor sets".into(),
    };
    for storage in [false, true] {
        let f = Fixture::new(contract());
        texture_resources(&f);
        texture_vertices(&f);
        texture_sampler(&f, 100, 200);
        f.instance
            .set_graphics_pipeline_cache(&cache, &identity, 2, 8)
            .unwrap();
        assert_eq!(
            f.instance
                .graphics_persistence_stats()
                .unwrap()
                .driver_cache_loaded,
            storage
        );
        f.instance
            .set_texture_contract(texture_contract(1))
            .unwrap();
        f.instance
            .set_uniform_buffer_contract(UniformBufferContract {
                bindings: vec![UniformBankMapping {
                    stage: 5,
                    index: 2,
                    target: UniformStage::Fragment,
                    bank: 2,
                }],
                storage_buffers: storage,
            })
            .unwrap();
        f.program(Arc::new(SourceShaders {
            vertex:compile_source("vert","#version 450\nlayout(location=0) in vec4 p; void main(){gl_Position=p;}"),
            fragment:compile_source("frag","#version 450\nlayout(set=1,binding=0) uniform texture2D t; layout(set=1,binding=1) uniform sampler s; layout(set=0,binding=2,std140) uniform Bank {vec4 data[4096];} bank; layout(location=0) out vec4 c; void main(){c=texture(sampler2D(t,s),gl_FragCoord.xy/32.0-vec2(0.5))*bank.data[1];}"),
        }),&shader(true,0.0),&shader(false,0.0));
        let draw = || {
            texture_draw_state(&f, false, false, false);
            call(
                &f.instance,
                "nvnCommandBufferBindUniformBuffer",
                &[7, 5, 2, f.base + 0xe000, 32],
            );
            call(
                &f.instance,
                "nvnCommandBufferDrawArrays",
                &[7, 0xf001, 1, 3],
            );
        };
        put(&f.state, 0xf000, &floats(&[0.0; 8]));
        draw();
        submit(&f.instance, &f.state, Status::Ok);
        wait_for_graphics(&f.instance);
        assert!(driver_file(&cache).is_file());
        for tint in [[1.0, 0.5, 0.25, 1.0], [0.25, 1.0, 0.5, 0.5]] {
            put(&f.state, 0xf010, &floats(&tint));
            draw();
            submit(&f.instance, &f.state, Status::Ok);
            let pixels = f.pixels();
            for y in 0..64 {
                for x in 0..64 {
                    let sample = checker_reference(
                        (x as f32 + 0.5) / 32.0 - 0.5,
                        (y as f32 + 0.5) / 32.0 - 0.5,
                        false,
                        200,
                    );
                    for c in 0..4 {
                        let expected = (f32::from(sample[c]) * tint[c]).round() as u8;
                        assert!(
                            pixels[(y * 64 + x) * 4 + c].abs_diff(expected) <= 1,
                            "texture bank storage {storage} at {x},{y}"
                        );
                    }
                }
            }
        }
        assert_eq!(f.instance.graphics_cache_stats().unwrap().misses, 1);
    }
    println!("MATCH texture descriptors beside stage-local uniform and storage banks, rebinding, pipeline reuse and driver-cache reopening");
}

#[path = "startup_drawing.rs"]
mod startup;

#[path = "pipeline_latency.rs"]
mod latency;
