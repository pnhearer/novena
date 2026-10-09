//! Original pattern images, sampled on the GPU and compared byte for byte. Provenance: 0031.
#![cfg(feature = "vulkan")]
use ash::vk;
use novena::{
    gpu::{
        image_layout::{format_block, BlitRegion, ImageDescriptor, ImageRegion},
        Backend, GlobalMemory,
    },
    tiling::{ImageKind, ImageShape, Layout, TileShape},
};
use std::{ffi::CString, fs, path::PathBuf, process::Command};

fn packing(shape: ImageShape, format: vk::Format, height: u8, depth: u8) -> Layout {
    Layout::new(
        shape,
        format_block(format).unwrap(),
        TileShape {
            height_log2: height,
            depth_log2: depth,
        },
    )
    .unwrap()
}
fn pattern(layout: &Layout) -> Vec<u8> {
    let mut bytes = vec![0; layout.linear_size()];
    for layer in 0..layout.shape().layers {
        for (level, p) in layout.levels().iter().enumerate() {
            for z in 0..p.blocks[2] {
                for y in 0..p.blocks[1] {
                    for x in 0..p.row_bytes {
                        let at = layout
                            .linear_byte_offset(level as u32, layer, x as u32, y, z)
                            .unwrap();
                        bytes[at] = ((x * 13
                            + y as usize * 29
                            + z as usize * 71
                            + level * 37
                            + layer as usize * 19)
                            % 251) as u8;
                    }
                }
            }
        }
    }
    bytes
}
fn assert_active(layout: &Layout, actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), layout.linear_size());
    for layer in 0..layout.shape().layers {
        for p in layout.levels() {
            let at = layer as usize * layout.linear_layer_stride() + p.linear_offset;
            let size = p.row_bytes * p.blocks[1] as usize * p.blocks[2] as usize;
            assert_eq!(&actual[at..at + size], &expected[at..at + size]);
        }
    }
}
fn sample_shader(kind: ImageKind, layers: u32) -> Vec<u32> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let shape = match kind {
        ImageKind::D2 => 0,
        ImageKind::D2Array => 1,
        ImageKind::D3 => 2,
        ImageKind::Cube if layers == 6 => 3,
        ImageKind::Cube => 4,
    };
    let output = root
        .join("../../target/tmp")
        .join(format!("sample-{shape}-{}.spv", std::process::id()));
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    let result = Command::new("glslangValidator")
        .args(["-V", "-g0", "--target-env", "vulkan1.2"])
        .arg(format!("-DSHAPE={shape}"))
        .arg("-o")
        .arg(&output)
        .arg(root.join("tests/shaders/sample_image.comp"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(Command::new("spirv-val")
        .args(["--target-env", "vulkan1.2"])
        .arg(&output)
        .status()
        .unwrap()
        .success());
    let bytes = fs::read(&output).unwrap();
    fs::remove_file(output).unwrap();
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect()
}
fn sampled(backend: &mut Backend, key: u64, layout: &Layout) -> Vec<u8> {
    let view = backend.sampled_image(key).expect("sampled view");
    let context = backend.context();
    let device = &context.device;
    let mut output =
        GlobalMemory::new(context, layout.linear_size().next_multiple_of(16) as u64).unwrap();
    let guest = output
        .allocate_pool(1, 0x1000, layout.linear_size() as u64)
        .unwrap();
    let address = output.addresses().host(guest).unwrap();
    let words = sample_shader(layout.shape().kind, layout.shape().layers);
    // All handles belong to this context and survive until the queue completes.
    unsafe {
        let sampler = device
            .create_sampler(
                &vk::SamplerCreateInfo::default()
                    .mag_filter(vk::Filter::NEAREST)
                    .min_filter(vk::Filter::NEAREST)
                    .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                    .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                    .max_lod(32.0),
                None,
            )
            .unwrap();
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::COMPUTE)];
        let set_layout = device
            .create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )
            .unwrap();
        let sets = [set_layout];
        let ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .size(32)];
        let pipeline_layout = device
            .create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&sets)
                    .push_constant_ranges(&ranges),
                None,
            )
            .unwrap();
        let sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 1,
        }];
        let descriptors = device
            .create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )
            .unwrap();
        let set = device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(descriptors)
                    .set_layouts(&sets),
            )
            .unwrap()[0];
        let images = [vk::DescriptorImageInfo::default()
            .image_view(view)
            .sampler(sampler)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        device.update_descriptor_sets(
            &[vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&images)],
            &[],
        );
        let module = context.create_global_shader_module(&words).unwrap();
        let entry = CString::new("main").unwrap();
        let pipeline = device
            .create_compute_pipelines(
                vk::PipelineCache::null(),
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(
                        vk::PipelineShaderStageCreateInfo::default()
                            .stage(vk::ShaderStageFlags::COMPUTE)
                            .module(module)
                            .name(&entry),
                    )
                    .layout(pipeline_layout)],
                None,
            )
            .unwrap()[0];
        let pool = device
            .create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(context.queue_family),
                None,
            )
            .unwrap();
        let cmd = device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
            .unwrap()[0];
        device
            .begin_command_buffer(cmd, &vk::CommandBufferBeginInfo::default())
            .unwrap();
        device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, pipeline);
        device.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::COMPUTE,
            pipeline_layout,
            0,
            &[set],
            &[],
        );
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)],
            &[],
            &[],
        );
        for layer in 0..layout.shape().layers {
            for (level, p) in layout.levels().iter().enumerate() {
                let mut push = Vec::from(address.to_ne_bytes());
                for value in [
                    p.extent[0],
                    p.extent[1],
                    p.extent[2],
                    layer,
                    level as u32,
                    (layer as usize * layout.linear_layer_stride() + p.linear_offset) as u32,
                ] {
                    push.extend(value.to_ne_bytes());
                }
                device.cmd_push_constants(
                    cmd,
                    pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    &push,
                );
                device.cmd_dispatch(
                    cmd,
                    (p.extent[0] * p.extent[1] * p.extent[2]).div_ceil(64),
                    1,
                    1,
                );
            }
        }
        device.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::HOST,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::HOST_READ)],
            &[],
            &[],
        );
        device.end_command_buffer(cmd).unwrap();
        device
            .queue_submit(
                context.queue,
                &[vk::SubmitInfo::default().command_buffers(&[cmd])],
                vk::Fence::null(),
            )
            .unwrap();
        device.queue_wait_idle(context.queue).unwrap();
        device.destroy_command_pool(pool, None);
        device.destroy_pipeline(pipeline, None);
        device.destroy_shader_module(module, None);
        device.destroy_descriptor_pool(descriptors, None);
        device.destroy_pipeline_layout(pipeline_layout, None);
        device.destroy_descriptor_set_layout(set_layout, None);
        device.destroy_sampler(sampler, None);
    }
    let mut bytes = vec![0; layout.linear_size()];
    output.read_pool(1, 0, &mut bytes).unwrap();
    bytes
}

#[test]
#[ignore = "requires a real Vulkan device and shader tools; never silently skips"]
fn tiled_mips_sample_exactly_and_preserve_padding() {
    let mut backend = Backend::new(1.0).expect("required GPU");
    backend.global_memory = Some(GlobalMemory::new(backend.context(), 8 * 1024 * 1024).unwrap());
    let mut cases = Vec::new();
    for h in [0, 1, 3, 5] {
        cases.push((
            ImageShape {
                width: 67,
                height: 35,
                depth: 1,
                layers: 1,
                levels: 7,
                kind: ImageKind::D2,
            },
            h,
            0,
        ));
    }
    cases.extend([
        (
            ImageShape {
                width: 19,
                height: 11,
                depth: 1,
                layers: 3,
                levels: 5,
                kind: ImageKind::D2Array,
            },
            2,
            0,
        ),
        (
            ImageShape {
                width: 17,
                height: 13,
                depth: 9,
                layers: 1,
                levels: 5,
                kind: ImageKind::D3,
            },
            2,
            2,
        ),
        (
            ImageShape {
                width: 17,
                height: 17,
                depth: 1,
                layers: 6,
                levels: 5,
                kind: ImageKind::Cube,
            },
            3,
            0,
        ),
        (
            ImageShape {
                width: 9,
                height: 9,
                depth: 1,
                layers: 12,
                levels: 4,
                kind: ImageKind::Cube,
            },
            1,
            0,
        ),
    ]);
    for (i, (shape, h, d)) in cases.into_iter().enumerate() {
        let layout = packing(shape, vk::Format::R8G8B8A8_UINT, h, d);
        let linear = pattern(&layout);
        let mut tiled = vec![0x9b; layout.tiled_size()];
        layout.encode(&linear, &mut tiled).unwrap();
        let key = i as u64 + 1;
        backend
            .allocate_pool(
                key,
                0x1000 + key * 0x100000,
                layout.tiled_size() as u64 + 32,
            )
            .unwrap();
        backend
            .global_memory
            .as_mut()
            .unwrap()
            .write_pool(key, 0, &vec![0x9b; layout.tiled_size() + 32])
            .unwrap();
        backend
            .global_memory
            .as_mut()
            .unwrap()
            .write_pool(key, 16, &tiled)
            .unwrap();
        assert!(backend.ensure_image(
            key,
            &ImageDescriptor {
                shape,
                format: vk::Format::R8G8B8A8_UINT
            }
        ));
        assert!(backend.load_tiled(key, key, 16, &layout));
        assert!(backend.wait_transfers());
        assert_active(&layout, &backend.readback(key).unwrap().2, &linear);
        assert_active(&layout, &sampled(&mut backend, key, &layout), &linear);
        assert!(backend.store_tiled(key, key, 16, &layout));
        assert!(backend.wait_transfers());
        let mut restored = vec![0; layout.tiled_size() + 32];
        backend
            .global_memory
            .as_mut()
            .unwrap()
            .read_pool(key, 0, &mut restored)
            .unwrap();
        assert_eq!(&restored[..16], &[0x9b; 16]);
        assert_eq!(&restored[16..16 + layout.tiled_size()], &tiled);
        assert_eq!(&restored[16 + layout.tiled_size()..], &[0x9b; 16]);
        backend.release_pool(key);
    }
}

#[test]
#[ignore = "requires a real Vulkan device; compressed support is queried"]
fn compressed_blocks_pass_through_all_mips() {
    let mut backend = Backend::new(1.0).expect("required GPU");
    backend.global_memory = Some(GlobalMemory::new(backend.context(), 8 * 1024 * 1024).unwrap());
    let mut formats = vec![
        vk::Format::BC1_RGB_UNORM_BLOCK,
        vk::Format::BC1_RGBA_SRGB_BLOCK,
        vk::Format::BC2_UNORM_BLOCK,
        vk::Format::BC3_UNORM_BLOCK,
        vk::Format::BC4_UNORM_BLOCK,
        vk::Format::BC4_SNORM_BLOCK,
        vk::Format::BC5_UNORM_BLOCK,
        vk::Format::BC5_SNORM_BLOCK,
        vk::Format::BC6H_UFLOAT_BLOCK,
        vk::Format::BC6H_SFLOAT_BLOCK,
        vk::Format::BC7_UNORM_BLOCK,
        vk::Format::BC7_SRGB_BLOCK,
    ];
    formats.extend(
        (vk::Format::ASTC_4X4_UNORM_BLOCK.as_raw()..=vk::Format::ASTC_12X12_SRGB_BLOCK.as_raw())
            .map(vk::Format::from_raw),
    );
    for (i, format) in formats.into_iter().enumerate() {
        let shape = ImageShape {
            width: 37,
            height: 29,
            depth: 1,
            layers: 2,
            levels: 6,
            kind: ImageKind::D2Array,
        };
        let layout = packing(shape, format, 3, 0);
        let properties = unsafe {
            backend
                .context()
                .instance
                .get_physical_device_format_properties(backend.context().physical_device, format)
        };
        let supported = properties.optimal_tiling_features.contains(
            vk::FormatFeatureFlags::SAMPLED_IMAGE
                | vk::FormatFeatureFlags::TRANSFER_SRC
                | vk::FormatFeatureFlags::TRANSFER_DST,
        );
        if !supported {
            assert!(
                format.as_raw() >= vk::Format::ASTC_4X4_UNORM_BLOCK.as_raw(),
                "BC format must be supported on this test device"
            );
            eprintln!("ASTC format {} unavailable", format.as_raw());
            continue;
        }
        let linear = pattern(&layout);
        let mut tiled = vec![0x69; layout.tiled_size()];
        layout.encode(&linear, &mut tiled).unwrap();
        let key = i as u64 + 100;
        backend
            .allocate_pool(key, key * 0x100000, layout.tiled_size() as u64)
            .unwrap();
        backend
            .global_memory
            .as_mut()
            .unwrap()
            .write_pool(key, 0, &tiled)
            .unwrap();
        assert!(backend.ensure_image(key, &ImageDescriptor { shape, format }));
        assert!(backend.load_tiled(key, key, 0, &layout));
        assert!(backend.wait_transfers());
        assert_active(&layout, &backend.readback(key).unwrap().2, &linear);
        assert!(backend.store_tiled(key, key, 0, &layout));
        assert!(backend.wait_transfers());
        let mut restored = vec![0; layout.tiled_size()];
        backend
            .global_memory
            .as_mut()
            .unwrap()
            .read_pool(key, 0, &mut restored)
            .unwrap();
        assert_eq!(restored, tiled);
        backend.release_pool(key);
    }
}

#[test]
#[ignore = "requires a real Vulkan device"]
fn image_regions_and_blits_have_exact_pixels() {
    let mut backend = Backend::new(1.0).expect("required GPU");
    let shape = ImageShape {
        width: 17,
        height: 13,
        depth: 1,
        layers: 3,
        levels: 5,
        kind: ImageKind::D2Array,
    };
    let layout = packing(shape, vk::Format::R8G8B8A8_UINT, 1, 0);
    let original = pattern(&layout);
    let zero = vec![0; layout.linear_size()];
    for key in [1, 2] {
        assert!(backend.ensure_image(
            key,
            &ImageDescriptor {
                shape,
                format: vk::Format::R8G8B8A8_UINT
            }
        ));
        assert!(backend.upload_image(key, if key == 1 { &original } else { &zero }));
    }
    assert!(backend.copy(2, 1));
    assert_active(&layout, &backend.readback(2).unwrap().2, &original);
    assert!(backend.upload_image(2, &vec![0; layout.linear_size()]));
    let from = ImageRegion {
        level: 1,
        layer: 2,
        offset: [1, 1, 0],
        extent: [3, 2, 1],
    };
    let to = ImageRegion {
        layer: 1,
        offset: [2, 2, 0],
        ..from
    };
    assert!(backend.copy_region(2, 1, from, to));
    let result = backend.readback(2).unwrap().2;
    for y in 0..2 {
        for x in 0..3 {
            let src = layout
                .linear_byte_offset(1, 2, (x + 1) * 4, y + 1, 0)
                .unwrap();
            let dst = layout
                .linear_byte_offset(1, 1, (x + 2) * 4, y + 2, 0)
                .unwrap();
            assert_eq!(&result[dst..dst + 4], &original[src..src + 4]);
        }
    }
    let small = ImageShape {
        width: 3,
        height: 2,
        depth: 1,
        layers: 1,
        levels: 1,
        kind: ImageKind::D2,
    };
    let large = ImageShape {
        width: 6,
        height: 4,
        ..small
    };
    let small_layout = packing(small, vk::Format::R8G8B8A8_UNORM, 0, 0);
    let large_layout = packing(large, vk::Format::R8G8B8A8_UNORM, 0, 0);
    let bytes = pattern(&small_layout);
    assert!(backend.ensure_image(
        3,
        &ImageDescriptor {
            shape: small,
            format: vk::Format::R8G8B8A8_UNORM
        }
    ));
    assert!(backend.ensure_image(
        4,
        &ImageDescriptor {
            shape: large,
            format: vk::Format::R8G8B8A8_UNORM
        }
    ));
    assert!(backend.upload_image(3, &bytes));
    assert!(backend.blit_region(
        4,
        3,
        BlitRegion {
            level: 0,
            layer: 0,
            offsets: [[0, 0, 0], [3, 2, 1]]
        },
        BlitRegion {
            level: 0,
            layer: 0,
            offsets: [[0, 0, 0], [6, 4, 1]]
        },
        vk::Filter::NEAREST
    ));
    let pixels = backend.readback(4).unwrap().2;
    for y in 0..4 {
        for x in 0..6 {
            let src = small_layout
                .linear_byte_offset(0, 0, (x / 2) * 4, y / 2, 0)
                .unwrap();
            let dst = large_layout.linear_byte_offset(0, 0, x * 4, y, 0).unwrap();
            assert_eq!(&pixels[dst..dst + 4], &bytes[src..src + 4]);
        }
    }
    assert!(!backend.copy_region(
        2,
        1,
        ImageRegion {
            offset: [u32::MAX, 0, 0],
            ..from
        },
        to
    ));
    assert!(!backend.copy_region(1, 1, from, from));
}

#[test]
#[ignore = "requires a real Vulkan device; exercises the recorded command executor"]
fn recorded_tiled_copies_and_blits_update_arena_bytes() {
    use novena::{
        functions,
        gpu::image_layout::{CopyOperation, ImageContract, ImageRule, Storage},
        Host, Instance, Registers, Status,
    };
    use std::{
        ffi::c_void,
        sync::{Arc, Mutex},
    };
    unsafe extern "C" fn read(user: *mut c_void, address: u64, out: *mut u8, size: u64) -> i32 {
        let memory = unsafe { &*user.cast::<Mutex<Vec<u8>>>() }.lock().unwrap();
        let Some(bytes) = memory.get(address as usize..address as usize + size as usize) else {
            return 1;
        };
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), out, bytes.len());
        }
        0
    }
    unsafe extern "C" fn write(
        user: *mut c_void,
        address: u64,
        bytes: *const u8,
        size: u64,
    ) -> i32 {
        let mut memory = unsafe { &*user.cast::<Mutex<Vec<u8>>>() }.lock().unwrap();
        let Some(out) = memory.get_mut(address as usize..address as usize + size as usize) else {
            return 1;
        };
        unsafe {
            std::ptr::copy_nonoverlapping(bytes, out.as_mut_ptr(), out.len());
        }
        0
    }
    fn call(i: &Instance, suffix: &str, args: &[u64]) -> u64 {
        let id = functions::all()
            .find(|(_, name)| name.ends_with(suffix))
            .unwrap()
            .0;
        let mut r = Registers::default();
        r.x[..args.len()].copy_from_slice(args);
        assert_eq!(i.call(id, &mut r), Status::Ok, "{suffix}");
        r.x[0]
    }
    let memory = Mutex::new(vec![0; 256 * 1024]);
    let host = Host {
        user: std::ptr::from_ref(&memory).cast_mut().cast(),
        read_memory: Some(read),
        write_memory: Some(write),
        render_scale: 1.0,
        ..Host::default()
    };
    // Callbacks and their storage stay live until this instance is destroyed.
    let instance = unsafe { Instance::with_host(host) };
    let tile = TileShape {
        height_log2: 2,
        depth_log2: 0,
    };
    instance
        .set_image_contract(ImageContract {
            rules: vec![ImageRule {
                flags: 0x400,
                target: 0x500,
                format: 0x600,
                host_format: vk::Format::R8G8B8A8_UINT,
                kind: ImageKind::D2Array,
                storage: Storage::Tiled(tile),
            }],
        })
        .expect("required GPU");
    let shape = ImageShape {
        width: 17,
        height: 11,
        depth: 1,
        layers: 2,
        levels: 5,
        kind: ImageKind::D2Array,
    };
    let layout = packing(shape, vk::Format::R8G8B8A8_UINT, 2, 0);
    let original = pattern(&layout);
    let span = layout.tiled_size();
    let pool_size = span * 3;
    call(&instance, "MemoryPoolBuilderSetDefaults", &[1]);
    call(
        &instance,
        "MemoryPoolBuilderSetStorage",
        &[1, 0x1000, pool_size as u64],
    );
    assert_eq!(call(&instance, "MemoryPoolInitialize", &[2, 1]), 1);
    for (key, offset) in [(4, 0), (5, span)] {
        call(&instance, "TextureBuilderSetDefaults", &[3]);
        call(&instance, "TextureBuilderSetFlags", &[3, 0x400]);
        call(&instance, "TextureBuilderSetTarget", &[3, 0x500]);
        call(&instance, "TextureBuilderSetFormat", &[3, 0x600]);
        call(&instance, "TextureBuilderSetSize3D", &[3, 17, 11, 2]);
        call(&instance, "TextureBuilderSetLevels", &[3, 5]);
        call(
            &instance,
            "TextureBuilderSetStorage",
            &[3, 2, offset as u64],
        );
        assert_eq!(
            call(&instance, "TextureBuilderGetStorageSize", &[3]),
            span as u64
        );
        assert_eq!(call(&instance, "TextureInitialize", &[key, 3]), 1);
    }
    call(&instance, "TextureViewSetDefaults", &[30]);
    call(&instance, "TextureViewSetLevels", &[30, 2, 1]);
    call(&instance, "TextureViewSetLayers", &[30, 1, 1]);
    assert_eq!(
        call(&instance, "TextureGetViewOffset", &[4, 30]),
        (layout.array_stride() + layout.levels()[2].tiled_offset) as u64
    );
    let mut source = vec![0xab; span];
    layout.encode(&original, &mut source).unwrap();
    {
        let mut bytes = memory.lock().unwrap();
        bytes[0x1000..0x1000 + pool_size].fill(0xab);
        bytes[0x1000..0x1000 + span].copy_from_slice(&source);
    }
    assert!(instance.set_image_copy_decoder(Some(Arc::new(|args| {
        assert_eq!(&args[3..7], &[0x111, 0x222, 0x333, 0x444]);
        let from = ImageRegion {
            level: 1,
            layer: 1,
            offset: [1, 1, 0],
            extent: [3, 2, 1],
        };
        let to = ImageRegion {
            level: 1,
            layer: 0,
            offset: [2, 2, 0],
            extent: [3, 2, 1],
        };
        Some(if args[7] == 0 {
            CopyOperation::Copy {
                source: args[1],
                destination: args[2],
                from,
                to,
            }
        } else {
            CopyOperation::Blit {
                source: args[1],
                destination: args[2],
                from: BlitRegion {
                    level: 1,
                    layer: 1,
                    offsets: [[0, 0, 0], [2, 2, 1]],
                },
                to: BlitRegion {
                    level: 0,
                    layer: 0,
                    offsets: [[0, 0, 0], [4, 4, 1]],
                },
                filter: vk::Filter::NEAREST,
            }
        })
    }))));
    call(&instance, "CommandBufferInitialize", &[7, 0]);
    for mode in [0, 1] {
        call(&instance, "CommandBufferBeginRecording", &[7]);
        call(
            &instance,
            "CommandBufferCopyTextureToTexture",
            &[7, 4, 5, 0x111, 0x222, 0x333, 0x444, mode],
        );
        let handle = call(&instance, "CommandBufferEndRecording", &[7]);
        memory.lock().unwrap()[0x100..0x108].copy_from_slice(&handle.to_le_bytes());
        call(&instance, "QueueSubmitCommands", &[0, 1, 0x100]);
        let bytes = memory.lock().unwrap();
        let mut decoded = vec![0; layout.linear_size()];
        layout
            .decode(&bytes[0x1000 + span..0x1000 + span * 2], &mut decoded)
            .unwrap();
        if mode == 0 {
            for y in 0..2 {
                for x in 0..3 {
                    let a = layout
                        .linear_byte_offset(1, 1, (x + 1) * 4, y + 1, 0)
                        .unwrap();
                    let b = layout
                        .linear_byte_offset(1, 0, (x + 2) * 4, y + 2, 0)
                        .unwrap();
                    assert_eq!(&decoded[b..b + 4], &original[a..a + 4]);
                }
            }
        } else {
            for y in 0..4 {
                for x in 0..4 {
                    let a = layout
                        .linear_byte_offset(1, 1, (x / 2) * 4, y / 2, 0)
                        .unwrap();
                    let b = layout.linear_byte_offset(0, 0, x * 4, y, 0).unwrap();
                    assert_eq!(&decoded[b..b + 4], &original[a..a + 4]);
                }
            }
        }
        assert_eq!(&bytes[0x1000..0x1000 + span], &source);
        assert!(bytes[0x1000 + span * 2..0x1000 + span * 3]
            .iter()
            .all(|b| *b == 0xab));
    }
}

#[test]
#[ignore = "required GPU"]
fn batched_transfers_replay_with_changed_bytes_and_preserve_guards() {
    let mut backend = Backend::new(1.0).expect("required GPU");
    backend.global_memory = Some(GlobalMemory::new(backend.context(), 4 * 1024 * 1024).unwrap());
    backend
        .global_memory
        .as_mut()
        .unwrap()
        .allocate_pool(1, 0x1000, 4 * 1024 * 1024)
        .unwrap();
    let layouts: Vec<_> = (0..12)
        .map(|i| {
            let height = [0, 2, 5][i % 3];
            packing(
                ImageShape {
                    width: 67,
                    height: 35,
                    depth: 1,
                    layers: 1,
                    levels: 7,
                    kind: ImageKind::D2,
                },
                vk::Format::R8G8B8A8_UINT,
                height,
                0,
            )
        })
        .collect();
    let resources: Vec<_> = layouts
        .iter()
        .enumerate()
        .map(|(i, layout)| {
            let key = 100 + i as u64;
            assert!(backend.ensure_image(
                key,
                &ImageDescriptor {
                    shape: layout.shape(),
                    format: vk::Format::R8G8B8A8_UINT,
                },
            ));
            (key, 1, 256 + i as u64 * 0x20000, layout)
        })
        .collect();
    let mut expected = Vec::new();
    for generation in 0..5 {
        expected.clear();
        for (i, &(_, pool, offset, layout)) in resources.iter().enumerate() {
            let mut linear = pattern(layout);
            for byte in &mut linear {
                *byte = byte.wrapping_add((generation * 17 + i * 31) as u8);
            }
            let mut tiled = vec![0xa5; layout.tiled_size() + 32];
            layout.encode(&linear, &mut tiled[16..]).unwrap();
            assert!(backend
                .global_memory
                .as_mut()
                .unwrap()
                .write_pool(pool, offset - 16, &tiled)
                .is_some());
            expected.push((linear, tiled));
        }
        assert!(backend.load_tiled_batch(&resources));
        assert!(backend.wait_transfers());
    }
    for (&(key, _, _, layout), (linear, _)) in resources.iter().zip(&expected) {
        assert_active(layout, &backend.readback(key).unwrap().2, linear);
    }
    for _ in 0..5 {
        for &(_, pool, offset, layout) in &resources {
            assert!(backend
                .global_memory
                .as_mut()
                .unwrap()
                .write_pool(pool, offset - 16, &vec![0xa5; layout.tiled_size() + 32])
                .is_some());
        }
        assert!(backend.store_tiled_batch(&resources));
        assert!(backend.wait_transfers());
    }
    for (&(_, pool, offset, layout), (_, tiled)) in resources.iter().zip(&expected) {
        let mut actual = vec![0; layout.tiled_size() + 32];
        backend
            .global_memory
            .as_mut()
            .unwrap()
            .read_pool(pool, offset - 16, &mut actual)
            .unwrap();
        assert_eq!(&actual, tiled);
    }
    for &(key, _, _, _) in &resources {
        backend.release_texture(key);
    }
    for &(key, _, _, layout) in &resources {
        assert!(backend.ensure_image(
            key,
            &ImageDescriptor {
                shape: layout.shape(),
                format: vk::Format::R8G8B8A8_UINT,
            }
        ));
    }
    assert!(backend.load_tiled_batch(&resources));
    assert!(backend.wait_transfers());
    for (&(key, _, _, layout), (linear, _)) in resources.iter().zip(&expected) {
        assert_active(layout, &backend.readback(key).unwrap().2, linear);
    }
    assert!(!backend.load_tiled_batch(&[]));
    assert!(!backend.load_tiled_batch(&[(100, 1, 4 * 1024 * 1024, &layouts[0])]));
}
