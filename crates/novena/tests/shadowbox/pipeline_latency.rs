//! First-request to completed pixel readback. Evidence: provenance 0037.
use super::*;
use std::time::{Duration, Instant};

#[test]
#[ignore = "fresh-process timing requires the translator and a Vulkan GPU"]
fn translated_pipeline_first_use() {
    const TEST: &str = "latency::translated_pipeline_first_use";
    if std::env::var_os("NOVENA_PIPELINE_TIMING_CHILD").is_some() {
        let f = Fixture::new(contract());
        if std::env::var_os("NOVENA_PIPELINE_TIMING_WAIT").is_some() {
            f.instance.set_pending_draw_policy(novena::gpu::graphics::PendingDrawPolicy::Wait(Duration::from_millis(2))).unwrap();
        }
        let pressure_root = std::env::var_os("NOVENA_PIPELINE_TIMING_PRESSURE").map(|_| {
            let root = std::env::temp_dir().join(format!("pipeline-pressure-{}", std::process::id()));
            f.instance.set_graphics_pipeline_cache(
                &root, &novena::gpu::pipelines::TranslationIdentity {
                    version: "synthetic pressure 1".into(), configuration: String::new(),
                }, 1, 1,
            ).unwrap();
            root
        });
        for (index, vertex) in [
            [-0.75, -0.75, 0.0, 1.0],
            [0.75, -0.75, 0.0, 1.0],
            [0.0, 0.75, 0.0, 1.0],
        ]
        .iter()
        .enumerate()
        {
            put(&f.state, 0x1080 + (index + 1) * 32 + 8, &floats(vertex));
        }
        assert!(f.instance.set_pending_draw_policy(novena::gpu::graphics::PendingDrawPolicy::Wait(Duration::from_millis(17))).is_err());
        let mut first_use = Vec::new();
        let mut spikes = Vec::new();
        for variant in 1..=24 {
            let green = variant as f32 / 32.0;
            f.program(
                Arc::new(Translator(AtomicUsize::new(0))),
                &shader(true, green),
                &shader(false, 0.0),
            );
            f.begin();
            submit(&f.instance, &f.state, Status::Ok);
            let center = 0x2000 + (24 * 64 + 32) * 4;
            let expected = (green * 255.0).round() as u8;
            let start = Instant::now();
            let mut spike = 0;
            loop {
                let frame = Instant::now();
                triangle(&f.instance, &f.state, f.base + 0x80, 128, 0xf001);
                submit(&f.instance, &f.state, Status::Ok);
                spike = spike.max(frame.elapsed().as_nanos());
                if f.state.memory.lock().unwrap()[center..center + 4]
                    == [255, expected, 128, 255]
                {
                    break;
                }
                assert!(start.elapsed() < Duration::from_secs(30), "draw timed out");
                std::thread::sleep(Duration::from_millis(1));
            }
            first_use.push(start.elapsed().as_nanos());
            spikes.push(spike);
        }
        assert_eq!(f.instance.graphics_cache_stats().unwrap().misses, 24);
        assert!(f.instance.take_graphics_cache_diagnostics().is_empty());
        let support = f.instance.graphics_library_support().unwrap();
        let compiled = f.instance.graphics_compilation_stats().unwrap();
        if support.0 {
            assert_eq!(compiled.library_parts, [1, 1, 24, 1]);
            assert_eq!(compiled.links, 24);
            assert_eq!(compiled.whole, 0);
        } else {
            assert_eq!(compiled.library_parts, [0; 4]);
            assert_eq!(compiled.whole, 24);
        }
        println!("SUPPORT libraries={} fast={} parts={:?} links={}", support.0, support.1, compiled.library_parts, compiled.links);
        first_use.sort_unstable();
        spikes.sort_unstable();
        println!(
            "LATENCY variants=24 first_p50_us={} first_p95_us={} first_max_us={} frame_p50_us={} frame_p95_us={} frame_max_us={}",
            first_use[12] / 1000, first_use[22] / 1000, first_use[23] / 1000,
            spikes[12] / 1000, spikes[22] / 1000, spikes[23] / 1000,
        );
        drop(f);
        if let Some(root) = pressure_root { fs::remove_dir_all(root).unwrap(); }
        return;
    }
    for trial in 0..5 {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--include-ignored", "--nocapture"])
            .env("NOVENA_PIPELINE_TIMING_CHILD", "1")
            .env("MESA_SHADER_CACHE_DISABLE", "true")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8(output.stdout).unwrap();
        println!("{}", stdout.lines().find(|line| line.starts_with("SUPPORT ")).unwrap());
        println!("trial={trial} {}", stdout.lines().find(|line| line.starts_with("LATENCY ")).unwrap());
    }
}
