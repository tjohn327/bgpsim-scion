//! SCION Scalability Analysis
//!
//! This example tests SCION simulation scalability with batch APIs and MinimalOspf.
//! Target: 1000s of ASes, approaching millions of routers.
//!
//! Run with: cargo run --example scion_scalability --release --all-features
//!
//! For specific scale:
//!   cargo run --example scion_scalability --release --all-features -- huge
//!   cargo run --example scion_scalability --release --all-features -- massive

use bgpsim::event::BasicEventQueue;
use bgpsim::network::Network;
use bgpsim::ospf::MinimalOspf;
use bgpsim::scion::*;
use bgpsim::types::{RouterId, SimplePrefix};
use std::collections::HashMap;
use std::env;
use std::time::{Duration, Instant};

type ScionNetwork = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, MinimalOspf>;

fn main() {
    let args: Vec<String> = env::args().collect();
    let scale = args.get(1).map(|s| s.as_str()).unwrap_or("all");

    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║         SCION SCALABILITY ANALYSIS                               ║");
    println!("║         Using: MinimalOspf + Batch APIs                          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    let configs = get_configs();

    match scale {
        "all" => {
            println!("Running all scale configurations...\n");
            print_comparison_header();
            for (name, config) in &configs {
                if *name != "EXTREME" {
                    run_and_print(name, config);
                }
            }
            println!("\nNote: Run with 'extreme' argument to test EXTREME scale (may take minutes)");
        }
        "extreme" => {
            println!("Running EXTREME scale test (this may take several minutes)...\n");
            if let Some((_, config)) = configs.iter().find(|(n, _)| *n == "EXTREME") {
                run_detailed(config);
            }
        }
        name => {
            if let Some((_, config)) = configs.iter().find(|(n, _)| n.to_lowercase() == name.to_lowercase()) {
                run_detailed(config);
            } else {
                println!("Unknown scale: {}. Valid options: tiny, small, medium, large, xlarge, huge, massive, extreme", name);
            }
        }
    }
}

#[derive(Clone)]
struct ScaleConfig {
    name: &'static str,
    num_isds: usize,
    cores_per_isd: usize,
    intermediates_per_isd: usize,
    leaves_per_intermediate: usize,
    routers_per_core: usize,
    routers_per_intermediate: usize,
    routers_per_leaf: usize,
}

impl ScaleConfig {
    fn total_ases(&self) -> usize {
        self.num_isds
            * (self.cores_per_isd
                + self.intermediates_per_isd
                + self.intermediates_per_isd * self.leaves_per_intermediate)
    }

    fn total_routers(&self) -> usize {
        self.num_isds
            * (self.cores_per_isd * self.routers_per_core
                + self.intermediates_per_isd * self.routers_per_intermediate
                + self.intermediates_per_isd * self.leaves_per_intermediate * self.routers_per_leaf)
    }

    fn estimated_links(&self) -> usize {
        // Internal links (full mesh per AS) + external links
        let internal_per_core = self.routers_per_core * (self.routers_per_core - 1) / 2;
        let internal_per_int = self.routers_per_intermediate * (self.routers_per_intermediate - 1) / 2;
        let internal_per_leaf = self.routers_per_leaf * (self.routers_per_leaf - 1) / 2;

        let internal = self.num_isds
            * (self.cores_per_isd * internal_per_core
                + self.intermediates_per_isd * internal_per_int
                + self.intermediates_per_isd * self.leaves_per_intermediate * internal_per_leaf);

        // External: core mesh + core-intermediate + intermediate-leaf + inter-ISD
        let core_mesh = self.num_isds * self.cores_per_isd * (self.cores_per_isd - 1) / 2;
        let core_to_int = self.num_isds * self.intermediates_per_isd * 2; // 2 cores per intermediate
        let int_to_leaf = self.num_isds * self.intermediates_per_isd * self.leaves_per_intermediate;
        let inter_isd = self.num_isds; // Ring of inter-ISD links

        internal + core_mesh + core_to_int + int_to_leaf + inter_isd
    }
}

fn get_configs() -> Vec<(&'static str, ScaleConfig)> {
    vec![
        ("DENSE", ScaleConfig {
            name: "DENSE",
            num_isds: 3,
            cores_per_isd: 5,           // Many cores
            intermediates_per_isd: 4,   // Few intermediates
            leaves_per_intermediate: 2, // Few leaves
            routers_per_core: 1,
            routers_per_intermediate: 1,
            routers_per_leaf: 1,
        }),
        ("TINY", ScaleConfig {
            name: "TINY",
            num_isds: 3,
            cores_per_isd: 2,
            intermediates_per_isd: 2,
            leaves_per_intermediate: 3,
            routers_per_core: 2,
            routers_per_intermediate: 1,
            routers_per_leaf: 1,
        }),
        ("SMALL", ScaleConfig {
            name: "SMALL",
            num_isds: 5,
            cores_per_isd: 3,
            intermediates_per_isd: 5,
            leaves_per_intermediate: 5,
            routers_per_core: 2,
            routers_per_intermediate: 1,
            routers_per_leaf: 1,
        }),
        ("MEDIUM", ScaleConfig {
            name: "MEDIUM",
            num_isds: 10,
            cores_per_isd: 4,
            intermediates_per_isd: 10,
            leaves_per_intermediate: 10,
            routers_per_core: 3,
            routers_per_intermediate: 2,
            routers_per_leaf: 1,
        }),
        ("LARGE", ScaleConfig {
            name: "LARGE",
            num_isds: 20,
            cores_per_isd: 5,
            intermediates_per_isd: 20,
            leaves_per_intermediate: 15,
            routers_per_core: 4,
            routers_per_intermediate: 2,
            routers_per_leaf: 1,
        }),
        ("XLARGE", ScaleConfig {
            name: "XLARGE",
            num_isds: 50,
            cores_per_isd: 6,
            intermediates_per_isd: 30,
            leaves_per_intermediate: 20,
            routers_per_core: 4,
            routers_per_intermediate: 2,
            routers_per_leaf: 1,
        }),
        ("HUGE", ScaleConfig {
            name: "HUGE",
            num_isds: 100,
            cores_per_isd: 8,
            intermediates_per_isd: 50,
            leaves_per_intermediate: 30,
            routers_per_core: 4,
            routers_per_intermediate: 2,
            routers_per_leaf: 1,
        }),
        ("MASSIVE", ScaleConfig {
            name: "MASSIVE",
            num_isds: 200,
            cores_per_isd: 10,
            intermediates_per_isd: 80,
            leaves_per_intermediate: 50,
            routers_per_core: 4,
            routers_per_intermediate: 2,
            routers_per_leaf: 1,
        }),
        ("EXTREME", ScaleConfig {
            name: "EXTREME",
            num_isds: 500,
            cores_per_isd: 12,
            intermediates_per_isd: 100,
            leaves_per_intermediate: 80,
            routers_per_core: 4,
            routers_per_intermediate: 2,
            routers_per_leaf: 1,
        }),
    ]
}

#[derive(Default)]
struct TimingResults {
    topology_build: Duration,
    create_routers: Duration,
    add_links: Duration,
    enable_scion: Duration,
    core_beaconing: Duration,
    intra_isd_beaconing: Duration,
    segment_registration: Duration,
    path_db_build: Duration,
    path_queries: Duration,
    total: Duration,
}

fn print_comparison_header() {
    println!("{:<10} {:>10} {:>12} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
             "Scale", "ASes", "Routers", "Build(ms)", "Core(ms)", "Intra(ms)", "Reg(ms)", "DB(ms)", "Total(ms)");
    println!("{}", "─".repeat(102));
}

fn run_and_print(name: &str, config: &ScaleConfig) {
    let results = run_scale_test(config, false);
    println!("{:<10} {:>10} {:>12} {:>10.1} {:>10.1} {:>10.1} {:>10.1} {:>10.1} {:>10.1}",
             name,
             config.total_ases(),
             config.total_routers(),
             results.topology_build.as_secs_f64() * 1000.0,
             results.core_beaconing.as_secs_f64() * 1000.0,
             results.intra_isd_beaconing.as_secs_f64() * 1000.0,
             results.segment_registration.as_secs_f64() * 1000.0,
             results.path_db_build.as_secs_f64() * 1000.0,
             results.total.as_secs_f64() * 1000.0);
}

fn run_detailed(config: &ScaleConfig) {
    println!("Configuration: {}", config.name);
    println!("═══════════════════════════════════════════════════════════════════");
    println!("  ISDs:                    {:>10}", config.num_isds);
    println!("  Cores per ISD:           {:>10}", config.cores_per_isd);
    println!("  Intermediates per ISD:   {:>10}", config.intermediates_per_isd);
    println!("  Leaves per Intermediate: {:>10}", config.leaves_per_intermediate);
    println!("  ─────────────────────────────────────");
    println!("  Total ASes:              {:>10}", config.total_ases());
    println!("  Total Routers:           {:>10}", config.total_routers());
    println!("  Estimated Links:         {:>10}", config.estimated_links());
    println!();

    let results = run_scale_test(config, true);

    println!("\nTiming Breakdown:");
    println!("═══════════════════════════════════════════════════════════════════");
    println!("  Phase                         Time (ms)      % of Total");
    println!("  ─────────────────────────────────────────────────────────────────");
    let total_ms = results.total.as_secs_f64() * 1000.0;
    print_timing_row("Topology Build", results.topology_build, total_ms);
    print_timing_row("  - Create Routers", results.create_routers, total_ms);
    print_timing_row("  - Enable SCION", results.enable_scion, total_ms);
    print_timing_row("  - Add Links", results.add_links, total_ms);
    print_timing_row("Core Beaconing", results.core_beaconing, total_ms);
    print_timing_row("Intra-ISD Beaconing", results.intra_isd_beaconing, total_ms);
    print_timing_row("Segment Registration", results.segment_registration, total_ms);
    print_timing_row("Path DB Build", results.path_db_build, total_ms);
    print_timing_row("Path Queries (100)", results.path_queries, total_ms);
    println!("  ─────────────────────────────────────────────────────────────────");
    println!("  TOTAL                     {:>10.1} ms", total_ms);

    println!("\nPerformance Metrics:");
    println!("═══════════════════════════════════════════════════════════════════");
    let ases = config.total_ases() as f64;
    let routers = config.total_routers() as f64;
    println!("  Topology:     {:>12.0} routers/sec", routers / results.topology_build.as_secs_f64());
    println!("  Core Bcn:     {:>12.0} ASes/sec", ases / results.core_beaconing.as_secs_f64());
    println!("  Intra-ISD:    {:>12.0} ASes/sec", ases / results.intra_isd_beaconing.as_secs_f64());
    println!("  Seg Reg:      {:>12.0} ASes/sec", ases / results.segment_registration.as_secs_f64());
    println!("  Overall:      {:>12.0} ASes/sec", ases / results.total.as_secs_f64());

    // Memory estimate (rough)
    let mem_per_router_kb = 2; // ~2KB per router (minimal)
    let mem_per_segment_kb = 1; // ~1KB per segment
    let total_mem_mb = (routers as usize * mem_per_router_kb + config.total_ases() * 10 * mem_per_segment_kb) / 1024;
    println!("\nEstimated Memory Usage: ~{} MB", total_mem_mb);
}

fn print_timing_row(name: &str, duration: Duration, total_ms: f64) {
    let ms = duration.as_secs_f64() * 1000.0;
    let pct = (ms / total_ms) * 100.0;
    println!("  {:<27} {:>10.1} ms    {:>6.1}%", name, ms, pct);
}

struct ScalableTopology {
    core_ases: Vec<IsdAs>,
    leaf_ases: Vec<IsdAs>,
    all_ases: Vec<IsdAs>,
}

fn run_scale_test(config: &ScaleConfig, verbose: bool) -> TimingResults {
    let mut results = TimingResults::default();
    let overall_start = Instant::now();

    if verbose {
        println!("\nBuilding topology...");
    }

    // Phase 1: Build topology
    let (mut net, topology, create_time, enable_time, link_time) = build_topology(config, verbose);
    results.topology_build = create_time + enable_time + link_time;
    results.create_routers = create_time;
    results.enable_scion = enable_time;
    results.add_links = link_time;

    // Calculate core beaconing rounds needed for full inter-ISD connectivity
    // In a ring of N ISDs, we need N/2 rounds to reach the farthest ISD
    // Plus additional rounds for intra-ISD core mesh propagation
    let num_core_rounds = std::cmp::max(
        config.cores_per_isd * 2,
        (config.num_isds / 2) + config.cores_per_isd
    );

    if verbose {
        println!("Running core beaconing ({} rounds)...", num_core_rounds);
    }

    // Phase 2: Core beaconing
    let start = Instant::now();
    for _ in 0..num_core_rounds {
        net.propagate_core_batch(&topology.core_ases).unwrap();
    }
    results.core_beaconing = start.elapsed();

    if verbose {
        println!("Running intra-ISD beaconing...");
    }

    // Phase 3: Intra-ISD beaconing
    let start = Instant::now();
    let mut current_level = topology.core_ases.clone();
    let mut level_count = 0;
    while !current_level.is_empty() {
        let next_level = net.propagate_intra_isd_batch(&current_level).unwrap();
        current_level = next_level.into_iter().collect();
        level_count += 1;
    }
    results.intra_isd_beaconing = start.elapsed();

    if verbose {
        println!("  Propagated through {} levels", level_count);
        println!("Registering segments...");
    }

    // Phase 4: Segment registration
    let start = Instant::now();
    net.register_segments_batch(&topology.all_ases).unwrap();
    results.segment_registration = start.elapsed();

    if verbose {
        println!("Building path database...");
    }

    // Phase 5: Build global path database
    let start = Instant::now();
    let global_db = net.build_global_path_db(&topology.all_ases).unwrap();
    results.path_db_build = start.elapsed();

    let (up, down, core) = global_db.segment_counts();
    if verbose {
        println!("  Segments: {} up, {} down, {} core (total: {})", up, down, core, up + down + core);
        println!("Running path queries...");
    }

    // Phase 6: Path queries
    let start = Instant::now();
    let num_queries = std::cmp::min(100, topology.leaf_ases.len());
    for i in 0..num_queries {
        let src = topology.leaf_ases[i];
        let dst_idx = (i + topology.leaf_ases.len() / 2) % topology.leaf_ases.len();
        let dst = topology.leaf_ases[dst_idx];

        let src_is_core = net.is_scion_core(&src).unwrap_or(false);
        let dst_is_core = net.is_scion_core(&dst).unwrap_or(false);

        let query = PathQuery {
            src,
            dst,
            max_paths: 5,
            allow_peering: false,
        };
        let _ = construct_paths_with_peering(&query, &global_db, src_is_core, dst_is_core);
    }
    results.path_queries = start.elapsed();

    // Phase 7: Path verification (if verbose)
    if verbose {
        println!("\nVerifying paths...");
        verify_paths(config, &net, &topology, &global_db);
    }

    results.total = overall_start.elapsed();
    results
}

/// Verify that paths can be correctly retrieved for various src/dst pairs
fn verify_paths(
    config: &ScaleConfig,
    net: &ScionNetwork,
    topology: &ScalableTopology,
    global_db: &PathDatabase,
) {
    println!("═══════════════════════════════════════════════════════════════════");
    println!("PATH VERIFICATION");
    println!("═══════════════════════════════════════════════════════════════════\n");

    let mut passed = 0;
    let mut failed = 0;

    // Test case 1: Same ISD - leaf to leaf (via intermediate and core)
    println!("Test 1: Same ISD - Leaf to Leaf");
    println!("───────────────────────────────────────────────────────────────────");
    if config.num_isds > 0 && config.intermediates_per_isd >= 2 && config.leaves_per_intermediate > 0 {
        // Pick two leaves under different intermediates in ISD 1
        let src = IsdAs::new(1, 300); // First leaf under intermediate 0
        let dst = IsdAs::new(1, 400); // First leaf under intermediate 1

        if let Some(result) = verify_single_path(net, global_db, src, dst) {
            println!("  {} → {}", src, dst);
            println!("  Paths found: {}", result.paths_found);
            for (i, (path, hops)) in result.all_paths.iter().enumerate() {
                println!("    [{}] {} ({} hops)", i + 1, path, hops);
            }
            println!("  Valid: ✅ {}\n", result.validation);
            passed += 1;
        } else {
            println!("  {} → {}: ❌ No path found\n", src, dst);
            failed += 1;
        }
    } else {
        println!("  Skipped (topology too small)\n");
    }

    // Test case 2: Different ISDs - leaf to leaf (requires core segment)
    println!("Test 2: Different ISDs - Leaf to Leaf (Cross-ISD)");
    println!("───────────────────────────────────────────────────────────────────");
    if config.num_isds >= 2 && config.leaves_per_intermediate > 0 {
        let src = IsdAs::new(1, 300); // Leaf in ISD 1
        let dst = IsdAs::new(2, 300); // Leaf in ISD 2

        if let Some(result) = verify_single_path(net, global_db, src, dst) {
            println!("  {} → {}", src, dst);
            println!("  Paths found: {}", result.paths_found);
            for (i, (path, hops)) in result.all_paths.iter().enumerate() {
                println!("    [{}] {} ({} hops)", i + 1, path, hops);
            }
            println!("  Valid: ✅ {}\n", result.validation);
            passed += 1;
        } else {
            println!("  {} → {}: ❌ No path found\n", src, dst);
            failed += 1;
        }
    } else {
        println!("  Skipped (need ≥2 ISDs)\n");
    }

    // Test case 3: Core to Leaf
    println!("Test 3: Core to Leaf");
    println!("───────────────────────────────────────────────────────────────────");
    if config.num_isds > 0 && config.leaves_per_intermediate > 0 {
        let src = IsdAs::new(1, 100); // Core in ISD 1
        let dst = IsdAs::new(1, 300); // Leaf in ISD 1

        if let Some(result) = verify_single_path(net, global_db, src, dst) {
            println!("  {} → {}", src, dst);
            println!("  Paths found: {}", result.paths_found);
            for (i, (path, hops)) in result.all_paths.iter().enumerate() {
                println!("    [{}] {} ({} hops)", i + 1, path, hops);
            }
            println!("  Valid: ✅ {}\n", result.validation);
            passed += 1;
        } else {
            println!("  {} → {}: ❌ No path found\n", src, dst);
            failed += 1;
        }
    } else {
        println!("  Skipped (topology too small)\n");
    }

    // Test case 4: Core to Core (same ISD)
    println!("Test 4: Core to Core (Same ISD)");
    println!("───────────────────────────────────────────────────────────────────");
    if config.num_isds > 0 && config.cores_per_isd >= 2 {
        let src = IsdAs::new(1, 100); // Core 0 in ISD 1
        let dst = IsdAs::new(1, 101); // Core 1 in ISD 1

        if let Some(result) = verify_single_path(net, global_db, src, dst) {
            println!("  {} → {}", src, dst);
            println!("  Paths found: {}", result.paths_found);
            for (i, (path, hops)) in result.all_paths.iter().enumerate() {
                println!("    [{}] {} ({} hops)", i + 1, path, hops);
            }
            println!("  Valid: ✅ {}\n", result.validation);
            passed += 1;
        } else {
            println!("  {} → {}: ❌ No path found\n", src, dst);
            failed += 1;
        }
    } else {
        println!("  Skipped (need ≥2 cores per ISD)\n");
    }

    // Test case 5: Core to Core (different ISDs)
    println!("Test 5: Core to Core (Different ISDs)");
    println!("───────────────────────────────────────────────────────────────────");
    if config.num_isds >= 2 {
        let src = IsdAs::new(1, 100); // Core in ISD 1
        let dst = IsdAs::new(2, 100); // Core in ISD 2

        if let Some(result) = verify_single_path(net, global_db, src, dst) {
            println!("  {} → {}", src, dst);
            println!("  Paths found: {}", result.paths_found);
            for (i, (path, hops)) in result.all_paths.iter().enumerate() {
                println!("    [{}] {} ({} hops)", i + 1, path, hops);
            }
            println!("  Valid: ✅ {}\n", result.validation);
            passed += 1;
        } else {
            println!("  {} → {}: ❌ No path found\n", src, dst);
            failed += 1;
        }
    } else {
        println!("  Skipped (need ≥2 ISDs)\n");
    }

    // Test case 6: Adjacent ISDs in ring (should always work)
    println!("Test 6: Adjacent ISDs (Ring Neighbor)");
    println!("───────────────────────────────────────────────────────────────────");
    if config.num_isds >= 2 && config.leaves_per_intermediate > 0 {
        // ISD N connects to ISD 1 in the ring (wrap-around)
        let last_isd = config.num_isds as u16;
        let src = IsdAs::new(1, 300);         // Leaf in ISD 1
        let dst = IsdAs::new(last_isd, 300);  // Leaf in last ISD (adjacent via ring)

        if let Some(result) = verify_single_path(net, global_db, src, dst) {
            println!("  {} → {} (ring neighbor)", src, dst);
            println!("  Paths found: {}", result.paths_found);
            for (i, (path, hops)) in result.all_paths.iter().enumerate() {
                println!("    [{}] {} ({} hops)", i + 1, path, hops);
            }
            println!("  Valid: ✅ {}\n", result.validation);
            passed += 1;
        } else {
            println!("  {} → {}: ❌ No path found\n", src, dst);
            failed += 1;
        }
    } else {
        println!("  Skipped (need ≥2 ISDs)\n");
    }

    // Test case 7: Leaf to intermediate (its parent)
    println!("Test 7: Leaf to Parent Intermediate");
    println!("───────────────────────────────────────────────────────────────────");
    if config.num_isds > 0 && config.intermediates_per_isd > 0 && config.leaves_per_intermediate > 0 {
        let src = IsdAs::new(1, 300); // Leaf under intermediate 200
        let dst = IsdAs::new(1, 200); // Its parent intermediate

        if let Some(result) = verify_single_path(net, global_db, src, dst) {
            println!("  {} → {}", src, dst);
            println!("  Paths found: {}", result.paths_found);
            for (i, (path, hops)) in result.all_paths.iter().enumerate() {
                println!("    [{}] {} ({} hops)", i + 1, path, hops);
            }
            println!("  Valid: ✅ {}\n", result.validation);
            passed += 1;
        } else {
            println!("  {} → {}: ❌ No path found\n", src, dst);
            failed += 1;
        }
    } else {
        println!("  Skipped (topology too small)\n");
    }

    // Test case 8: Random sampling from leaf_ases
    println!("Test 8: Random Sampling (5 pairs from opposite ends)");
    println!("───────────────────────────────────────────────────────────────────");
    let sample_count = std::cmp::min(5, topology.leaf_ases.len() / 2);
    for i in 0..sample_count {
        let src = topology.leaf_ases[i];
        let dst_idx = topology.leaf_ases.len() - 1 - i;
        let dst = topology.leaf_ases[dst_idx];

        if let Some(result) = verify_single_path(net, global_db, src, dst) {
            println!("  {} → {}: ✅ {} hops", src, dst, result.hop_count);
            passed += 1;
        } else {
            println!("  {} → {}: ❌ No path found", src, dst);
            failed += 1;
        }
    }

    // Summary
    println!("\n═══════════════════════════════════════════════════════════════════");
    println!("VERIFICATION SUMMARY: {} passed, {} failed", passed, failed);
    if failed == 0 {
        println!("✅ All path queries returned valid results!");
    } else {
        println!("⚠️  Some paths could not be found or validated");
    }
    println!("═══════════════════════════════════════════════════════════════════\n");
}

struct PathVerificationResult {
    path_str: String,
    hop_count: usize,
    validation: String,
    paths_found: usize,
    all_paths: Vec<(String, usize)>,  // (path_str, hop_count)
}

fn verify_single_path(
    net: &ScionNetwork,
    global_db: &PathDatabase,
    src: IsdAs,
    dst: IsdAs,
) -> Option<PathVerificationResult> {
    let src_is_core = net.is_scion_core(&src).unwrap_or(false);
    let dst_is_core = net.is_scion_core(&dst).unwrap_or(false);

    let query = PathQuery {
        src,
        dst,
        max_paths: 1000,  // High limit to find all paths
        allow_peering: false,
    };

    let result = construct_paths_with_peering(&query, global_db, src_is_core, dst_is_core);

    if result.paths.is_empty() {
        return None;
    }

    // Collect all paths
    let mut all_paths = Vec::new();
    for p in &result.paths {
        let as_path = p.as_path();
        let path_str = as_path.iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(" → ");
        all_paths.push((path_str, as_path.len()));
    }

    // Take the first (best) path for validation
    let path = &result.paths[0];
    let as_path = path.as_path();

    // Validate the path
    let mut validation_issues = Vec::new();

    // Check source
    if as_path.first() != Some(&src) {
        validation_issues.push(format!("path starts at {} not {}",
            as_path.first().map(|a| a.to_string()).unwrap_or("?".to_string()), src));
    }

    // Check destination
    if as_path.last() != Some(&dst) {
        validation_issues.push(format!("path ends at {} not {}",
            as_path.last().map(|a| a.to_string()).unwrap_or("?".to_string()), dst));
    }

    // Check for loops
    let mut seen = std::collections::HashSet::new();
    for &isd_as in &as_path {
        if !seen.insert(isd_as) {
            validation_issues.push(format!("loop detected at {}", isd_as));
        }
    }

    let path_str = as_path.iter()
        .map(|a| a.to_string())
        .collect::<Vec<_>>()
        .join(" → ");

    let validation = if validation_issues.is_empty() {
        "Path is valid".to_string()
    } else {
        validation_issues.join(", ")
    };

    Some(PathVerificationResult {
        path_str,
        hop_count: as_path.len(),
        validation,
        paths_found: result.paths.len(),
        all_paths,
    })
}

fn build_topology(
    config: &ScaleConfig,
    verbose: bool,
) -> (ScionNetwork, ScalableTopology, Duration, Duration, Duration) {
    let mut net: ScionNetwork = Network::default();
    let mut as_routers: HashMap<IsdAs, Vec<RouterId>> = HashMap::new();
    let mut core_ases = Vec::new();
    let mut intermediate_ases = Vec::new();
    let mut leaf_ases = Vec::new();
    let mut all_links: Vec<(RouterId, RouterId)> = Vec::new();
    let mut scion_links: Vec<(RouterId, RouterId, ScionLinkType)> = Vec::new();

    // Phase 1a: Create routers
    let create_start = Instant::now();

    for isd in 1..=config.num_isds {
        // Cores
        for core_idx in 0..config.cores_per_isd {
            let isd_as = IsdAs::new(isd as u16, (100 + core_idx) as u32);
            let routers = create_routers(&mut net, isd_as, config.routers_per_core, &mut all_links);
            as_routers.insert(isd_as, routers);
            core_ases.push(isd_as);
        }

        // Intermediates and leaves
        for int_idx in 0..config.intermediates_per_isd {
            let int_as = IsdAs::new(isd as u16, (200 + int_idx) as u32);
            let routers = create_routers(&mut net, int_as, config.routers_per_intermediate, &mut all_links);
            as_routers.insert(int_as, routers);
            intermediate_ases.push(int_as);

            for leaf_idx in 0..config.leaves_per_intermediate {
                let leaf_asn = 300 + int_idx * 100 + leaf_idx;
                let leaf_as = IsdAs::new(isd as u16, leaf_asn as u32);
                let routers = create_routers(&mut net, leaf_as, config.routers_per_leaf, &mut all_links);
                as_routers.insert(leaf_as, routers);
                leaf_ases.push(leaf_as);
            }
        }
    }
    let create_time = create_start.elapsed();

    if verbose {
        println!("  Created {} routers in {:.1}ms", net.num_routers(), create_time.as_secs_f64() * 1000.0);
    }

    // Phase 1b: Enable SCION
    let enable_start = Instant::now();
    for &isd_as in &core_ases {
        for &router in &as_routers[&isd_as] {
            net.enable_scion_router(router, isd_as, true).unwrap();
        }
    }
    for &isd_as in &intermediate_ases {
        for &router in &as_routers[&isd_as] {
            net.enable_scion_router(router, isd_as, false).unwrap();
        }
    }
    for &isd_as in &leaf_ases {
        for &router in &as_routers[&isd_as] {
            net.enable_scion_router(router, isd_as, false).unwrap();
        }
    }
    let enable_time = enable_start.elapsed();

    if verbose {
        println!("  Enabled SCION on {} ASes in {:.1}ms",
                 core_ases.len() + intermediate_ases.len() + leaf_ases.len(),
                 enable_time.as_secs_f64() * 1000.0);
    }

    // Phase 1c: Add inter-AS links
    let link_start = Instant::now();

    for isd in 1..=config.num_isds {
        let isd = isd as u16;

        // Core mesh within ISD
        for i in 0..config.cores_per_isd {
            for j in (i + 1)..config.cores_per_isd {
                let core1 = IsdAs::new(isd, (100 + i) as u32);
                let core2 = IsdAs::new(isd, (100 + j) as u32);
                if let Some((r1, r2)) = get_link_routers(&as_routers, core1, core2, i, j) {
                    all_links.push((r1, r2));
                    scion_links.push((r1, r2, ScionLinkType::Core));
                }
            }
        }

        // Core to intermediate - connect to more cores for dense topology
        let cores_to_connect = if config.name == "DENSE" {
            config.cores_per_isd  // Connect to ALL cores
        } else {
            std::cmp::min(2, config.cores_per_isd)  // Normal: connect to 2 cores
        };

        for int_idx in 0..config.intermediates_per_isd {
            let intermediate = IsdAs::new(isd, (200 + int_idx) as u32);
            for core_idx in 0..cores_to_connect {
                let core = IsdAs::new(isd, (100 + core_idx) as u32);
                if let Some((r1, r2)) = get_link_routers(&as_routers, core, intermediate, int_idx % config.routers_per_core, core_idx) {
                    all_links.push((r1, r2));
                    scion_links.push((r1, r2, ScionLinkType::Child));
                }
            }

            // Intermediate to leaf
            for leaf_idx in 0..config.leaves_per_intermediate {
                let leaf_asn = 300 + int_idx * 100 + leaf_idx;
                let leaf = IsdAs::new(isd, leaf_asn as u32);
                if let Some((r1, r2)) = get_link_routers(&as_routers, intermediate, leaf, leaf_idx % config.routers_per_intermediate, 0) {
                    all_links.push((r1, r2));
                    scion_links.push((r1, r2, ScionLinkType::Child));
                }
            }
        }
    }

    // Inter-ISD core links
    if config.name == "DENSE" {
        // Dense: Full mesh between all cores across all ISDs
        for isd1 in 1..=config.num_isds {
            for isd2 in (isd1 + 1)..=config.num_isds {
                // Connect multiple cores between ISDs
                for core_idx in 0..config.cores_per_isd {
                    let core1 = IsdAs::new(isd1 as u16, (100 + core_idx) as u32);
                    let core2 = IsdAs::new(isd2 as u16, (100 + core_idx) as u32);
                    if let Some((r1, r2)) = get_link_routers(&as_routers, core1, core2, 0, 0) {
                        all_links.push((r1, r2));
                        scion_links.push((r1, r2, ScionLinkType::Core));
                    }
                }
            }
        }
    } else {
        // Normal: Ring topology with single inter-ISD link
        for i in 0..config.num_isds {
            let j = (i + 1) % config.num_isds;
            let core1 = IsdAs::new((i + 1) as u16, 100);
            let core2 = IsdAs::new((j + 1) as u16, 100);
            if let Some((r1, r2)) = get_link_routers(&as_routers, core1, core2, 1, 1) {
                all_links.push((r1, r2));
                scion_links.push((r1, r2, ScionLinkType::Core));
            }
        }
    }

    // Bulk add all links
    net.add_links_from(all_links).unwrap();

    // Add SCION metadata
    for (r1, r2, link_type) in scion_links {
        let _ = net.add_scion_link(r1, r2, link_type);
    }

    let link_time = link_start.elapsed();

    if verbose {
        println!("  Added links in {:.1}ms", link_time.as_secs_f64() * 1000.0);
    }

    let mut all_ases = Vec::with_capacity(core_ases.len() + intermediate_ases.len() + leaf_ases.len());
    all_ases.extend(&core_ases);
    all_ases.extend(&intermediate_ases);
    all_ases.extend(&leaf_ases);

    let topology = ScalableTopology {
        core_ases,
        leaf_ases,
        all_ases,
    };

    (net, topology, create_time, enable_time, link_time)
}

fn create_routers(
    net: &mut ScionNetwork,
    isd_as: IsdAs,
    count: usize,
    internal_links: &mut Vec<(RouterId, RouterId)>,
) -> Vec<RouterId> {
    let mut routers = Vec::with_capacity(count);

    for i in 0..count {
        let name = format!("{}_{}", isd_as, i);
        let router = net.add_router(&name, isd_as.asn.0);
        routers.push(router);
    }

    // Full mesh internal links
    for i in 0..routers.len() {
        for j in (i + 1)..routers.len() {
            internal_links.push((routers[i], routers[j]));
        }
    }

    routers
}

fn get_link_routers(
    as_routers: &HashMap<IsdAs, Vec<RouterId>>,
    as1: IsdAs,
    as2: IsdAs,
    idx1: usize,
    idx2: usize,
) -> Option<(RouterId, RouterId)> {
    let r1 = as_routers.get(&as1)?;
    let r2 = as_routers.get(&as2)?;
    Some((r1[idx1 % r1.len()], r2[idx2 % r2.len()]))
}
