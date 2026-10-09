//! Synthetic Maxwell programs translated by Shadowbox and executed by Novena.
//! Encoding sources: envytools envydis/gm107.c, cited in provenance 0023.

use ash::vk;
use novena::{
    global_memory::{ARENA_SIZE, GUEST_BASE},
    gpu::{Context, GlobalMemory},
};
use shadowbox::{PUSH_GLOBAL_DELTA_OFFSET, PUSH_GLOBAL_DELTA_SIZE};
use std::{ffi::CString, fs, process::Command, sync::Arc};

const BYTES: usize = 128;
const ALWAYS: u64 = 7 << 16;
// gm107.c:1925,2057,2170: EXIT, NOP and MOV32I with an unconditional guard.
const EXIT: u64 = 0xe300_0000_0000_0000 | ALWAYS;
const NOP: u64 = 0x50b0_0000_0000_0000 | ALWAYS;

fn mov(register: u8, value: u32) -> u64 {
    // Mesa sm50.rs:1938-1941 sets all four lane-mask bits for an immediate move.
    0x0100_0000_0000_0000 | ALWAYS | (15 << 12) | u64::from(register) | u64::from(value) << 20
}

// gm107.c:203-205,316,331,583-590,642-650,1898-1899.
fn global(store: bool, size: u8, data: u8, address: u8, offset: i32, extended: bool) -> u64 {
    let opcode = if store { 0xeed8_u64 } else { 0xeed0_u64 };
    opcode << 48
        | u64::from(size) << 48
        | ALWAYS
        | u64::from(data)
        | u64::from(address) << 8
        | u64::from(offset as u32 & 0x00ff_ffff) << 20
        | u64::from(extended) << 45
}

// gm107.c:321,339,673-694,718-737,1897,1903,1905.
fn atomic(opcode: u64, data: u8, address: u8, offset: i32, extended: bool) -> u64 {
    opcode
        | ALWAYS
        | u64::from(data)
        | u64::from(address) << 8
        | u64::from(offset as u32 & 0x000f_ffff) << 28
        | u64::from(extended) << 48
}

fn addresses(source: u64, destination: u64) -> Vec<u64> {
    vec![
        mov(2, source as u32),
        mov(3, (source >> 32) as u32),
        mov(4, destination as u32),
        mov(5, (destination >> 32) as u32),
    ]
}

fn program_bytes(instructions: &[u64]) -> Vec<u8> {
    // gm107.c:2174-2182,2215-2216: one control word per three instructions.
    let mut slots = instructions.to_vec();
    slots.push(EXIT);
    let mut code = Vec::new();
    for bundle in slots.chunks(3) {
        code.extend(0_u64.to_le_bytes());
        for slot in 0..3 {
            code.extend(bundle.get(slot).copied().unwrap_or(NOP).to_le_bytes());
        }
    }
    let mut program = vec![0; 80];
    program.extend(code);
    program
}

fn translate(instructions: &[u64]) -> Vec<u32> {
    shadowbox::translate_header_prefixed(&program_bytes(instructions))
        .expect("synthetic Maxwell translation")
        .spirv
}

struct Case {
    name: String,
    instructions: Vec<u64>,
    input: [[u8; BYTES]; 2],
    expected: [[u8; BYTES]; 2],
    workgroups: u32,
}

fn cases(source: u64, destination: u64) -> Vec<Case> {
    let mut cases = Vec::new();
    for extended in [false, true] {
        for offset in [-16, 0, 16] {
            for size in 0..=6 {
                let mut input = [[0x5a; BYTES], [0xa5; BYTES]];
                input[0][32..48].copy_from_slice(&[
                    0x81, 0xf2, 0xe3, 0xd4, 0xc5, 0xb6, 0xa7, 0x98, 0x89, 0x7a, 0x6b, 0x5c, 0x4d,
                    0x3e, 0x2f, 0x10,
                ]);
                let width = match size {
                    0 | 1 => 1,
                    2 | 3 => 2,
                    4 => 4,
                    5 => 8,
                    6 => 16,
                    _ => unreachable!(),
                };
                // Narrow accesses exercise interior byte/halfword addresses.
                let position = match size {
                    0 | 1 => 33,
                    2 | 3 => 34,
                    _ => 32,
                };
                let mut expected = input;
                expected[1][position..position + width]
                    .copy_from_slice(&input[0][position..position + width]);
                let mut instructions = addresses(
                    (source + position as u64).wrapping_add_signed(-i64::from(offset)),
                    (destination + position as u64).wrapping_add_signed(-i64::from(offset)),
                );
                instructions.push(global(false, size, 8, 2, offset, extended));
                instructions.push(global(true, size, 8, 4, offset, extended));
                if size < 4 {
                    // Also store the full register to check sign/zero extension.
                    let value = match size {
                        0 => u32::from(input[0][position]),
                        1 => i32::from(input[0][position] as i8) as u32,
                        2 => u32::from(u16::from_le_bytes(
                            input[0][position..position + 2].try_into().unwrap(),
                        )),
                        3 => i32::from(i16::from_le_bytes(
                            input[0][position..position + 2].try_into().unwrap(),
                        )) as u32,
                        _ => unreachable!(),
                    };
                    expected[1][64..68].copy_from_slice(&value.to_le_bytes());
                    instructions.extend(addresses(source, destination));
                    instructions.push(global(true, 4, 8, 4, 64, extended));
                }
                cases.push(Case {
                    name: format!("LDG/STG size={size} E={extended} offset={offset}"),
                    instructions,
                    input,
                    expected,
                    workgroups: 1,
                });
            }
        }
    }

    let mut input = [[0x5a; BYTES], [0xa5; BYTES]];
    input[0][..8].copy_from_slice(&(destination + 32).to_le_bytes());
    input[1][32..48].copy_from_slice(&[
        1, 2, 3, 4, 0x91, 0x92, 0x93, 0x94, 9, 10, 11, 12, 0xa1, 0xa2, 0xa3, 0xa4,
    ]);
    let mut expected = input;
    expected[0][64..80].copy_from_slice(&input[1][32..48]);
    let mut instructions = addresses(source, source);
    // The first load overwrites its own address pair with a guest pointer.
    instructions.push(global(false, 5, 2, 2, 0, true));
    instructions.push(global(false, 6, 8, 2, 0, true));
    instructions.push(global(true, 6, 8, 4, 64, true));
    cases.push(Case {
        name: "pointer loaded from another pool, overlapping address/result".into(),
        instructions,
        input,
        expected,
        workgroups: 1,
    });

    for extended in [false, true] {
        for offset in [-16, 16] {
            for signed in [false, true] {
                for reduction in [false, true] {
                    for operation in [0, 1, 2, 5, 6, 7, 8] {
                        if reduction && operation == 8 {
                            continue;
                        }
                        let old = 0x8000_0020_u32;
                        let operand = 0x7fff_fff1_u32;
                        let updated = match operation {
                            0 => old.wrapping_add(operand),
                            1 if signed => (old as i32).min(operand as i32) as u32,
                            1 => old.min(operand),
                            2 if signed => (old as i32).max(operand as i32) as u32,
                            2 => old.max(operand),
                            5 => old & operand,
                            6 => old | operand,
                            7 => old ^ operand,
                            8 => operand,
                            _ => unreachable!(),
                        };
                        let mut input = [[0x5a; BYTES], [0xa5; BYTES]];
                        input[0][32..36].copy_from_slice(&old.to_le_bytes());
                        let mut expected = input;
                        expected[0][32..36].copy_from_slice(&updated.to_le_bytes());
                        let mut instructions = addresses(
                            (source + 32).wrapping_add_signed(-i64::from(offset)),
                            destination,
                        );
                        instructions.push(mov(8, operand));
                        let opcode = if reduction {
                            0xebf8_0000_0000_0000 | operation << 23 | u64::from(signed) << 20
                        } else {
                            0xed00_0000_0000_0000
                                | operation << 52
                                | u64::from(signed) << 49
                                | 8 << 20
                        };
                        instructions.push(atomic(
                            opcode,
                            if reduction { 8 } else { 10 },
                            2,
                            offset,
                            extended,
                        ));
                        if !reduction {
                            instructions.push(global(true, 4, 10, 4, 32, extended));
                            expected[1][32..36].copy_from_slice(&old.to_le_bytes());
                        }
                        cases.push(Case {
                            name: format!(
                                "{} op={operation} signed={signed} E={extended} offset={offset}",
                                if reduction { "RED" } else { "ATOM" }
                            ),
                            instructions,
                            input,
                            expected,
                            workgroups: 1,
                        });
                    }
                }
            }
            for success in [false, true] {
                let old = 0x1234_abcd_u32;
                let replacement = 0xdec0_de01_u32;
                let mut input = [[0x5a; BYTES], [0xa5; BYTES]];
                input[0][32..36].copy_from_slice(&old.to_le_bytes());
                let mut expected = input;
                if success {
                    expected[0][32..36].copy_from_slice(&replacement.to_le_bytes());
                }
                expected[1][32..36].copy_from_slice(&old.to_le_bytes());
                let mut instructions = addresses(
                    (source + 32).wrapping_add_signed(-i64::from(offset)),
                    destination,
                );
                instructions.push(mov(8, if success { old } else { old ^ 1 }));
                instructions.push(mov(9, replacement));
                instructions.push(atomic(
                    0xeef0_0000_0000_0000 | 8 << 20,
                    10,
                    2,
                    offset,
                    extended,
                ));
                instructions.push(global(true, 4, 10, 4, 32, extended));
                cases.push(Case {
                    name: format!("CAS success={success} E={extended} offset={offset}"),
                    instructions,
                    input,
                    expected,
                    workgroups: 1,
                });
            }
        }
    }

    let mut input = [[0x5a; BYTES], [0xa5; BYTES]];
    input[0][32..36].copy_from_slice(&7_u32.to_le_bytes());
    let mut expected = input;
    expected[0][32..36].copy_from_slice(&(7 + 64 * 3_u32).to_le_bytes());
    let mut instructions = addresses(source, destination);
    instructions.push(mov(8, 3));
    instructions.push(atomic(0xebf8_0000_0000_0000, 8, 2, 32, true));
    cases.push(Case {
        name: "RED ADD contention from 64 workgroups".into(),
        instructions,
        input,
        expected,
        workgroups: 64,
    });
    cases
}

fn check_contract(words: &[u32]) {
    assert_eq!(PUSH_GLOBAL_DELTA_OFFSET, 0);
    assert_eq!(PUSH_GLOBAL_DELTA_SIZE, 8);
    let range = GlobalMemory::push_constant_range(vk::ShaderStageFlags::COMPUTE);
    assert_eq!(range.offset, PUSH_GLOBAL_DELTA_OFFSET);
    assert_eq!(range.size, PUSH_GLOBAL_DELTA_SIZE);
    // Public SPIR-V opcodes from spirv.h, listed in Shadowbox's contract.
    let mut at = 5;
    let mut push_variables = Vec::new();
    let mut struct_members = Vec::new();
    let mut pointers = Vec::new();
    let mut offsets = Vec::new();
    let mut u64_types = Vec::new();
    let mut local_size = false;
    while at < words.len() {
        let count = (words[at] >> 16) as usize;
        let operands = &words[at + 1..at + count];
        match words[at] & 0xffff {
            16 if operands[1..] == [17, 1, 1, 1] => local_size = true,
            21 if operands[1..] == [64, 0] => u64_types.push(operands[0]),
            30 => struct_members.push(operands),
            32 if operands[1] == 9 => pointers.push(operands),
            59 if operands[2] == 9 => push_variables.push(operands[0]),
            72 if operands[1..] == [0, 35, PUSH_GLOBAL_DELTA_OFFSET] => {
                offsets.push(operands[0]);
            }
            _ => {}
        }
        at += count;
    }
    assert!(local_size, "one invocation per workgroup");
    assert_eq!(push_variables.len(), 1, "exactly one push-constant block");
    let pointer = pointers.iter().find(|p| p[0] == push_variables[0]).unwrap();
    let structure = struct_members.iter().find(|s| s[0] == pointer[2]).unwrap();
    assert_eq!(structure.len(), 2, "exactly one delta member");
    assert!(u64_types.contains(&structure[1]), "delta must be uint64");
    assert!(offsets.contains(&structure[0]), "delta member offset");
}

#[test]
fn synthetic_translations_match_novena_push_constants() {
    let cases = cases(GUEST_BASE, GUEST_BASE + BYTES as u64);
    assert_eq!(cases.len(), 156);
    for case in cases {
        check_contract(&translate(&case.instructions));
    }
}

fn validate(words: &[u32]) {
    let path = std::env::temp_dir().join(format!("novena-shadowbox-{}.spv", std::process::id()));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let result = Command::new("spirv-val")
        .args(["--target-env", "vulkan1.2"])
        .arg(&path)
        .output()
        .expect("spirv-val must be installed");
    fs::remove_file(path).unwrap();
    assert!(
        result.status.success(),
        "SPIR-V: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn dispatch(context: &Context, memory: &GlobalMemory, words: &[u32], workgroups: u32) {
    let module = context.create_global_shader_module(words).unwrap();
    let stages = vk::ShaderStageFlags::COMPUTE;
    let ranges = [GlobalMemory::push_constant_range(stages)];
    let device = &context.device;
    // SAFETY: objects belong to this context, the validated shader addresses live
    // arena pools, and all GPU work finishes before readback or object destruction.
    unsafe {
        let layout = device
            .create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&ranges),
                None,
            )
            .unwrap();
        let entry = CString::new("main").unwrap();
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(stages)
            .module(module)
            .name(&entry);
        let pipeline = device
            .create_compute_pipelines(
                vk::PipelineCache::null(),
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(stage)
                    .layout(layout)],
                None,
            )
            .unwrap()[0];
        let pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(context.queue_family),
                None,
            )
            .unwrap();
        let command = device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
            .unwrap()[0];
        device
            .begin_command_buffer(command, &vk::CommandBufferBeginInfo::default())
            .unwrap();
        device.cmd_bind_pipeline(command, vk::PipelineBindPoint::COMPUTE, pipeline);
        memory.push_delta(command, layout, stages);
        device.cmd_dispatch(command, workgroups, 1, 1);
        let barrier = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ);
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::HOST,
            vk::DependencyFlags::empty(),
            &[barrier],
            &[],
            &[],
        );
        device.end_command_buffer(command).unwrap();
        device
            .queue_submit(
                context.queue,
                &[vk::SubmitInfo::default().command_buffers(&[command])],
                vk::Fence::null(),
            )
            .unwrap();
        device.queue_wait_idle(context.queue).unwrap();
        device.destroy_command_pool(pool, None);
        device.destroy_pipeline(pipeline, None);
        device.destroy_pipeline_layout(layout, None);
        device.destroy_shader_module(module, None);
    }
}

#[test]
#[ignore = "requires a Vulkan GPU, 1 GiB coherent device-address memory and spirv-val; never skips"]
fn shadowbox_programs_execute_in_novena_arena() {
    let context = Arc::new(Context::new().expect("Vulkan flat global memory features"));
    let mut memory = GlobalMemory::new(&context, ARENA_SIZE).expect("1 GiB arena");
    let source = memory.allocate_pool(1, 0x1000, BYTES as u64).unwrap();
    let destination = memory.allocate_pool(2, 0x2000, BYTES as u64).unwrap();
    assert_eq!(source, 0x1_0000);
    assert_eq!(destination, 0x1_0080);
    let map = memory.addresses();
    assert_eq!(map.size, 1 << 30);
    assert_eq!(map.delta(), map.host_base.wrapping_sub(map.guest_base));
    assert_ne!(
        map.delta(),
        0,
        "exercise the default nonzero-delta lowering"
    );
    assert_eq!(map.delta() % 16, 0);
    for guest in [source, destination] {
        assert_eq!(map.host(guest).unwrap(), guest.wrapping_add(map.delta()));
    }
    let cases = cases(source, destination);
    let count = cases.len();
    for case in cases {
        let words = translate(&case.instructions);
        check_contract(&words);
        validate(&words);
        for (index, bytes) in case.input.iter().enumerate() {
            memory.write_pool(index as u64 + 1, 0, bytes).unwrap();
        }
        dispatch(&context, &memory, &words, case.workgroups);
        for (index, expected) in case.expected.iter().enumerate() {
            let mut actual = [0; BYTES];
            memory.read_pool(index as u64 + 1, 0, &mut actual).unwrap();
            assert_eq!(&actual, expected, "{} pool={}", case.name, index + 1);
        }
        println!("MATCH {}", case.name);
    }
    assert_eq!(memory.addresses(), map, "arena delta remains fixed");
    println!("{count} Shadowbox/Novena GPU cases MATCH");
}

struct PipelineTranslator(std::sync::atomic::AtomicUsize);
impl novena::ShaderTranslator for PipelineTranslator {
    fn translate(&self, _: novena::ShaderStage, program: &[u8]) -> Result<Vec<u32>, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let output =
            shadowbox::translate_header_prefixed(program).map_err(|error| error.to_string())?;
        if output.requires_subgroup_size_32 {
            return Err("subgroup size 32 is not enabled by this executor".into());
        }
        validate(&output.spirv);
        Ok(output.spirv)
    }
}

#[test]
#[ignore = "requires Vulkan, the 1 GiB arena and spirv-val; never skips"]
fn compute_pipeline_cache_executes_translated_program() {
    use novena::gpu::pipelines::{
        AsyncComputePipelines, CacheStats, ComputePipelines, PipelineStatus,
    };
    use std::sync::atomic::Ordering;

    let context = Arc::new(Context::new().expect("compute Vulkan context"));
    let mut memory = GlobalMemory::new(&context, ARENA_SIZE).expect("flat arena");
    let source = memory.allocate_pool(1, 0x1000, BYTES as u64).unwrap();
    let destination = memory.allocate_pool(2, 0x2000, BYTES as u64).unwrap();
    let translator = Arc::new(PipelineTranslator(std::sync::atomic::AtomicUsize::new(0)));
    let mut cache = ComputePipelines::new(&context, translator.clone()).unwrap();
    let mut input = [0x5a; BYTES];
    input[32..36].copy_from_slice(&0x1234_abcd_u32.to_le_bytes());
    let mut instructions = addresses(source, destination);
    instructions.push(global(false, 4, 8, 2, 32, true));
    instructions.push(global(true, 4, 8, 4, 32, true));
    let program = program_bytes(&instructions);
    let first = cache.get_or_compile(&program).unwrap();
    // Distinct allocation with the same source, never object-address identity.
    let second = cache.get_or_compile(&program.clone()).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(translator.0.load(Ordering::Relaxed), 1);
    assert_eq!(cache.stats(), CacheStats { hits: 1, misses: 1 });
    assert!(
        first.bindings().is_empty(),
        "physical accesses have no descriptor"
    );
    instructions[4] = global(false, 4, 8, 2, 36, true);
    let changed = cache.get_or_compile(&program_bytes(&instructions)).unwrap();
    assert!(!Arc::ptr_eq(&first, &changed));
    assert_eq!(translator.0.load(Ordering::Relaxed), 2);
    assert_eq!(cache.stats(), CacheStats { hits: 1, misses: 2 });
    // Mesa SM50 OpLdc: bank bits 36..41, byte offset 20..36,
    // dynamic offset register 8..16, B32 type 4 at 48..51. See provenance 0025.
    let ldc = (0xef90_u64 << 48) | (4 << 48) | ALWAYS | 8 | (255 << 8) | (32 << 20) | (3 << 36);
    let uniform_program = program_bytes(&[
        mov(4, destination as u32),
        mov(5, (destination >> 32) as u32),
        ldc,
        global(true, 4, 8, 4, 32, true),
    ]);
    let mut pool = AsyncComputePipelines::new(cache, 2, 8).unwrap();
    let uniform_request = pool.request(&uniform_program).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let uniform_pipeline = loop {
        match uniform_request.poll() {
            PipelineStatus::Ready(pipeline) => break pipeline,
            PipelineStatus::Failed(error) => panic!("{error}"),
            _ => assert!(std::time::Instant::now() < deadline, "worker timed out"),
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    let same = match pool.request(&uniform_program).unwrap().poll() {
        PipelineStatus::Ready(pipeline) => pipeline,
        _ => panic!("completed pipeline must be reused"),
    };
    assert!(Arc::ptr_eq(&uniform_pipeline, &same));
    let bindings = uniform_pipeline.bindings();
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        (bindings[0].set, bindings[0].binding, bindings[0].count),
        (0, 3, 1)
    );
    assert_eq!(
        bindings[0].descriptor_type.as_raw(),
        vk::DescriptorType::UNIFORM_BUFFER.as_raw()
    );
    assert_eq!(translator.0.load(Ordering::Relaxed), 3);
    assert_eq!(pool.stats(), CacheStats { hits: 2, misses: 3 });
    // Pipeline ownership is independent of the cache and its workers.
    drop(pool);
    memory.write_pool(1, 0, &input).unwrap();
    memory.write_pool(2, 0, &[0xa5; BYTES]).unwrap();
    submit_pipeline(&context, &memory, &first, &[]);
    let mut actual = [0; BYTES];
    memory.read_pool(2, 0, &mut actual).unwrap();
    let mut expected = [0xa5; BYTES];
    expected[32..36].copy_from_slice(&input[32..36]);
    assert_eq!(
        actual, expected,
        "translated compute copied exactly one word"
    );
    memory.write_pool(2, 0, &[0xa5; BYTES]).unwrap();
    let buffer = memory.uniform_buffer_info(1, 0, BYTES as u64).unwrap();
    assert!(memory.uniform_buffer_info(1, 0, 0).is_none());
    assert!(memory.uniform_buffer_info(1, 0, BYTES as u64 + 1).is_none());
    // SAFETY: the descriptor pool and live arena belong to this context. The
    // set is populated from a bounded pool slice and destroyed after completion.
    unsafe {
        let device = &context.device;
        let sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(1)];
        let pool = device
            .create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )
            .unwrap();
        let sets = device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(uniform_pipeline.set_layouts()),
            )
            .unwrap();
        device.update_descriptor_sets(
            &[vk::WriteDescriptorSet::default()
                .dst_set(sets[0])
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&[buffer])],
            &[],
        );
        submit_pipeline(&context, &memory, &uniform_pipeline, &sets);
        device.destroy_descriptor_pool(pool, None);
    }
    memory.read_pool(2, 0, &mut actual).unwrap();
    assert_eq!(
        actual, expected,
        "translated LDC reads the reflected uniform binding"
    );
    println!("MATCH translated physical and uniform compute dispatches, content cache hit and changed-content miss");
}

fn submit_pipeline(
    context: &Context,
    memory: &GlobalMemory,
    pipeline: &novena::gpu::pipelines::ComputePipeline,
    sets: &[vk::DescriptorSet],
) {
    // SAFETY: this test exclusively uses the queue and retains all referenced
    // pipelines, descriptors and live pools until completion.
    unsafe {
        let device = &context.device;
        let pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(context.queue_family),
                None,
            )
            .unwrap();
        let command = device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
            .unwrap()[0];
        device
            .begin_command_buffer(command, &vk::CommandBufferBeginInfo::default())
            .unwrap();
        if !sets.is_empty() {
            assert!(pipeline
                .record_dispatch(command, memory, &[], [1, 1, 1])
                .is_err());
        }
        let limits = context
            .instance
            .get_physical_device_properties(context.physical_device)
            .limits;
        if let Some((axis, over)) = limits
            .max_compute_work_group_count
            .iter()
            .enumerate()
            .find_map(|(axis, count)| count.checked_add(1).map(|over| (axis, over)))
        {
            let mut groups = [1; 3];
            groups[axis] = over;
            assert!(pipeline
                .record_dispatch(command, memory, sets, groups)
                .is_err());
        }
        pipeline
            .record_dispatch(command, memory, sets, [1, 1, 1])
            .unwrap();
        device.end_command_buffer(command).unwrap();
        device
            .queue_submit(
                context.queue,
                &[vk::SubmitInfo::default().command_buffers(&[command])],
                vk::Fence::null(),
            )
            .unwrap();
        device.queue_wait_idle(context.queue).unwrap();
        device.destroy_command_pool(pool, None);
    }
}
