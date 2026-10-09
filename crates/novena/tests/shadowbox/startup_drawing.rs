//! Fresh-process startup measurements using original synthetic programs.
use super::*;
#[path = "../support/startup_translator.rs"]
mod adapter;
use adapter::Adapter;
use novena::startup_cache::StartupCacheConfig;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
#[ignore = "fresh-process timing requires the translator and a Vulkan GPU"]
fn cold_and_warm_first_draw() {
    const TEST: &str = "startup::cold_and_warm_first_draw";
    if let Some(root) = std::env::var_os("NOVENA_STARTUP_TIMING_ROOT") {
        let root = PathBuf::from(root);
        let warm = std::env::var("NOVENA_STARTUP_TIMING_MODE").unwrap() == "warm";
        let fragment = shader(true, 0.25);
        let vertex = shader(false, 0.0);
        let start = Instant::now();
        let translator = Arc::new(Adapter(AtomicUsize::new(0)));
        let f = Fixture::with_instance(contract(), |host| unsafe {
            Instance::with_host_startup_cache(
                host,
                StartupCacheConfig::new(root),
                translator.clone(),
            )
            .unwrap()
        });
        let construction_us = start.elapsed().as_micros();
        let loaded = f.instance.startup_cache_stats().unwrap();
        assert_eq!(loaded.loaded, if warm { 2 } else { 0 });
        assert_eq!(loaded.pipelines_queued, u64::from(warm));
        let driver_loaded = f
            .instance
            .graphics_persistence_stats()
            .unwrap()
            .driver_cache_loaded;
        assert_eq!(driver_loaded, warm);
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
        let registration_start = Instant::now();
        f.program(translator.clone(), &fragment, &vertex);
        let registration_us = registration_start.elapsed().as_micros();
        f.begin();
        submit(&f.instance, &f.state, Status::Ok);
        let center = 0x2000 + (24 * 64 + 32) * 4;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            triangle(&f.instance, &f.state, f.base + 0x80, 128, 0xf001);
            submit(&f.instance, &f.state, Status::Ok);
            if f.state.memory.lock().unwrap()[center..center + 4] == [255, 64, 128, 255] {
                break;
            }
            assert!(Instant::now() < deadline, "first draw timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
        let elapsed_us = start.elapsed().as_micros();
        check_pixels(&f.state.memory.lock().unwrap()[0x2000..0x6000], 64);
        let stats = f.instance.startup_cache_stats().unwrap();
        assert_eq!(stats.hits, if warm { 2 } else { 0 });
        assert_eq!(stats.misses, if warm { 0 } else { 2 });
        assert_eq!(stats.translated, if warm { 0 } else { 2 });
        assert_eq!(
            translator.0.load(Ordering::Relaxed),
            if warm { 0 } else { 2 }
        );
        assert_eq!(f.instance.graphics_cache_stats().unwrap().misses, 1);
        assert!(f.instance.take_startup_cache_diagnostics().is_empty());
        assert!(f.instance.take_graphics_cache_diagnostics().is_empty());
        println!(
            "TIMING mode={} first_draw_us={elapsed_us} construction_us={construction_us} registration_us={registration_us} translation_us={} hits={} misses={} pipelines_queued={} driver_loaded={driver_loaded}",
            if warm { "warm" } else { "cold" }, stats.translation_nanoseconds / 1000,
            stats.hits, stats.misses, stats.pipelines_queued,
        );
        return;
    }

    let root = std::env::temp_dir().join(format!(
        "novena-startup-timing-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    fs::create_dir_all(&root).unwrap();
    let child = |cache: &std::path::Path, mode: &str| {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--include-ignored", "--nocapture"])
            .env("NOVENA_STARTUP_TIMING_ROOT", cache)
            .env("NOVENA_STARTUP_TIMING_MODE", mode)
            .env("MESA_SHADER_CACHE_DISABLE", "true")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        stdout
            .lines()
            .find(|line| line.starts_with("TIMING "))
            .unwrap()
            .to_owned()
    };
    const TRIALS: usize = 40;
    let mut timings = [Vec::new(), Vec::new()];
    for trial in 0..TRIALS {
        let cache = root.join(trial.to_string());
        let warm_cache = cache.join("warm");
        let cold_cache = cache.join("cold");
        fs::create_dir_all(&warm_cache).unwrap();
        fs::create_dir(&cold_cache).unwrap();
        // Seed only the warm application cache in an untimed fresh process.
        child(&warm_cache, "cold");
        for index in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
            let mode = ["cold", "warm"][index];
            let row = child(if index == 0 { &cold_cache } else { &warm_cache }, mode);
            println!("trial={trial} {row}");
            let elapsed: u128 = row
                .split_whitespace()
                .find_map(|word| word.strip_prefix("first_draw_us="))
                .unwrap()
                .parse()
                .unwrap();
            timings[index].push(elapsed);
        }
        fs::remove_dir_all(cache).unwrap();
    }
    for (index, mode) in ["cold", "warm"].into_iter().enumerate() {
        let values = &mut timings[index];
        values.sort_unstable();
        let median = |slice: &[u128]| (slice[slice.len() / 2 - 1] + slice[slice.len() / 2]) / 2;
        println!(
            "SUMMARY mode={mode} trials={TRIALS} median_us={} q1_us={} q3_us={} min_us={} max_us={}",
            median(values), median(&values[..TRIALS / 2]), median(&values[TRIALS / 2..]),
            values[0], values[TRIALS - 1],
        );
    }
    fs::remove_dir_all(root).unwrap();
}
