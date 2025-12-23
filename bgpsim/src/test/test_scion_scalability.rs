// SCION Scalability Benchmark
//
// This test measures performance characteristics for large-scale SCION simulations.
// Run with: cargo test --all-features test_scion_scalability -- --nocapture --ignored

use crate::event::BasicEventQueue;
use crate::network::Network;
use crate::ospf::MinimalOspf;
use crate::scion::*;
use crate::types::{RouterId, SimplePrefix};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Type alias for SCION network using MinimalOspf (no SPT computation overhead)
type ScionNetwork = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, MinimalOspf>;

/// Configuration for scalability tests
struct ScaleConfig {
    num_isds: usize,
    cores_per_isd: usize,
    intermediates_per_isd: usize,
    leaves_per_intermediate: usize,
    border_routers_per_core: usize,
    border_routers_per_intermediate: usize,
    border_routers_per_leaf: usize,
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
            * (self.cores_per_isd * self.border_routers_per_core
                + self.intermediates_per_isd * self.border_routers_per_intermediate
                + self.intermediates_per_isd
                    * self.leaves_per_intermediate
                    * self.border_routers_per_leaf)
    }
}

/// Timing results for each phase
#[derive(Default)]
struct TimingResults {
    topology_build: Duration,
    topology_create_as: Duration,
    topology_add_links: Duration,
    core_beaconing: Duration,
    intra_isd_beaconing: Duration,
    segment_registration: Duration,
    path_db_build: Duration,
    path_queries: Duration,
    total: Duration,
}

impl TimingResults {
    fn print(&self, config: &ScaleConfig) {
        println!("\n=== Scalability Results ===");
        println!("Configuration:");
        println!("  ISDs: {}", config.num_isds);
        println!("  Cores/ISD: {}", config.cores_per_isd);
        println!("  Intermediates/ISD: {}", config.intermediates_per_isd);
        println!("  Leaves/Intermediate: {}", config.leaves_per_intermediate);
        println!("  Total ASes: {}", config.total_ases());
        println!("  Total Routers: {}", config.total_routers());
        println!();
        println!("Timing:");
        println!("  Topology build:       {:>10.3} ms", self.topology_build.as_secs_f64() * 1000.0);
        println!("    - Create ASes:      {:>10.3} ms", self.topology_create_as.as_secs_f64() * 1000.0);
        println!("    - Add links:        {:>10.3} ms", self.topology_add_links.as_secs_f64() * 1000.0);
        println!("  Core beaconing:       {:>10.3} ms", self.core_beaconing.as_secs_f64() * 1000.0);
        println!("  Intra-ISD beaconing:  {:>10.3} ms", self.intra_isd_beaconing.as_secs_f64() * 1000.0);
        println!("  Segment registration: {:>10.3} ms", self.segment_registration.as_secs_f64() * 1000.0);
        println!("  Path DB build:        {:>10.3} ms", self.path_db_build.as_secs_f64() * 1000.0);
        println!("  Path queries:         {:>10.3} ms", self.path_queries.as_secs_f64() * 1000.0);
        println!("  ----------------------------------------");
        println!("  TOTAL:                {:>10.3} ms", self.total.as_secs_f64() * 1000.0);

        // Performance metrics
        let ases = config.total_ases() as f64;
        let routers = config.total_routers() as f64;
        println!();
        println!("Performance Metrics:");
        println!("  Topology build:      {:>10.1} routers/sec", routers / self.topology_build.as_secs_f64());
        println!("  Core beaconing:      {:>10.1} ASes/sec", ases / self.core_beaconing.as_secs_f64());
        println!("  Intra-ISD beaconing: {:>10.1} ASes/sec", ases / self.intra_isd_beaconing.as_secs_f64());
        println!("  Total:               {:>10.1} ASes/sec", ases / self.total.as_secs_f64());
    }
}

/// Build topology structure
struct ScalableTopology {
    core_ases: Vec<IsdAs>,
    intermediate_ases: Vec<IsdAs>,
    leaf_ases: Vec<IsdAs>,
    all_ases: Vec<IsdAs>,
    as_routers: HashMap<IsdAs, Vec<RouterId>>,
}

/// Run the scalability benchmark
fn run_scale_test(config: ScaleConfig) -> TimingResults {
    let mut results = TimingResults::default();
    let overall_start = Instant::now();

    // Phase 1: Build topology
    let start = Instant::now();
    let (mut net, topology, create_as_time, add_links_time) = build_scalable_topology(&config);
    results.topology_build = start.elapsed();
    results.topology_create_as = create_as_time;
    results.topology_add_links = add_links_time;

    // Phase 2: Core beaconing
    let start = Instant::now();
    let num_core_rounds = config.cores_per_isd * 2; // Mesh diameter
    for _round in 0..num_core_rounds {
        net.propagate_core_batch(&topology.core_ases).unwrap();
    }
    results.core_beaconing = start.elapsed();

    // Phase 3: Intra-ISD beaconing (level-by-level)
    let start = Instant::now();
    let mut current_level: Vec<IsdAs> = topology.core_ases.clone();
    while !current_level.is_empty() {
        let next_level = net.propagate_intra_isd_batch(&current_level).unwrap();
        current_level = next_level.into_iter().collect();
    }
    results.intra_isd_beaconing = start.elapsed();

    // Phase 4: Segment registration
    let start = Instant::now();
    net.register_segments_batch(&topology.all_ases).unwrap();
    results.segment_registration = start.elapsed();

    // Phase 5: Build global path database
    let start = Instant::now();
    let global_db = net.build_global_path_db(&topology.all_ases).unwrap();
    results.path_db_build = start.elapsed();

    // Phase 6: Path queries (sample)
    let start = Instant::now();
    let num_queries = std::cmp::min(100, topology.leaf_ases.len());
    for i in 0..num_queries {
        let src = topology.leaf_ases[i];
        let dst_idx = (i + topology.leaf_ases.len() / 2) % topology.leaf_ases.len();
        let dst = topology.leaf_ases[dst_idx];

        let src_is_core = net.scion_services[&src].is_core;
        let dst_is_core = net.scion_services[&dst].is_core;

        let query = PathQuery {
            src,
            dst,
            max_paths: 5,
            allow_peering: false,
        };
        let _ = construct_paths_with_peering(&query, &global_db, src_is_core, dst_is_core);
    }
    results.path_queries = start.elapsed();

    results.total = overall_start.elapsed();

    // Print segment statistics
    let (up, down, core) = global_db.segment_counts();
    println!("\nSegment Statistics:");
    println!("  Up segments: {}", up);
    println!("  Down segments: {}", down);
    println!("  Core segments: {}", core);
    println!("  Total: {}", up + down + core);

    results
}

/// Link info for deferred creation
struct ScionLinkInfo {
    r1: RouterId,
    r2: RouterId,
    link_type: ScionLinkType,
}

/// Build a scalable topology based on configuration
/// Returns (network, topology, create_as_time, add_links_time)
fn build_scalable_topology(
    config: &ScaleConfig,
) -> (ScionNetwork, ScalableTopology, Duration, Duration) {
    let mut net: ScionNetwork = Network::default();
    let mut as_routers: HashMap<IsdAs, Vec<RouterId>> = HashMap::new();
    let mut core_ases = Vec::new();
    let mut intermediate_ases = Vec::new();
    let mut leaf_ases = Vec::new();
    let mut all_links: Vec<(RouterId, RouterId)> = Vec::new();
    let mut scion_links: Vec<ScionLinkInfo> = Vec::new();

    let create_as_start = Instant::now();

    // Create ASes for each ISD (collect internal links, don't add yet)
    for isd in 1..=config.num_isds {
        // Create core ASes
        for core_idx in 0..config.cores_per_isd {
            let asn = 100 + core_idx;
            let isd_as = IsdAs::new(isd as u16, asn as u32);
            let (routers, internal_links) = create_as(&mut net, isd_as, config.border_routers_per_core, true);
            all_links.extend(internal_links);
            as_routers.insert(isd_as, routers);
            core_ases.push(isd_as);
        }

        // Create intermediate ASes
        for int_idx in 0..config.intermediates_per_isd {
            let asn = 200 + int_idx;
            let isd_as = IsdAs::new(isd as u16, asn as u32);
            let (routers, internal_links) =
                create_as(&mut net, isd_as, config.border_routers_per_intermediate, false);
            all_links.extend(internal_links);
            as_routers.insert(isd_as, routers);
            intermediate_ases.push(isd_as);

            // Create leaf ASes under this intermediate
            for leaf_idx in 0..config.leaves_per_intermediate {
                let leaf_asn = 300 + int_idx * 100 + leaf_idx;
                let leaf_as = IsdAs::new(isd as u16, leaf_asn as u32);
                let (routers, internal_links) = create_as(&mut net, leaf_as, config.border_routers_per_leaf, false);
                all_links.extend(internal_links);
                as_routers.insert(leaf_as, routers);
                leaf_ases.push(leaf_as);
            }
        }
    }
    let create_as_time = create_as_start.elapsed();

    // Collect all external links
    let add_links_start = Instant::now();
    for isd in 1..=config.num_isds {
        let isd = isd as u16;

        // Core mesh within ISD
        for i in 0..config.cores_per_isd {
            for j in (i + 1)..config.cores_per_isd {
                let core1 = IsdAs::new(isd, 100 + i as u32);
                let core2 = IsdAs::new(isd, 100 + j as u32);
                if let Some((r1, r2)) = get_link_routers(&as_routers, core1, core2,
                    i % config.border_routers_per_core, j % config.border_routers_per_core) {
                    all_links.push((r1, r2));
                    scion_links.push(ScionLinkInfo { r1, r2, link_type: ScionLinkType::Core });
                }
            }
        }

        // Core to intermediate links
        for int_idx in 0..config.intermediates_per_isd {
            let intermediate = IsdAs::new(isd, 200 + int_idx as u32);
            for core_idx in 0..std::cmp::min(2, config.cores_per_isd) {
                let core = IsdAs::new(isd, 100 + core_idx as u32);
                let core_router_idx = (int_idx + 2) % config.border_routers_per_core;
                let int_router_idx = core_idx % config.border_routers_per_intermediate;
                if let Some((r1, r2)) = get_link_routers(&as_routers, core, intermediate,
                    core_router_idx, int_router_idx) {
                    all_links.push((r1, r2));
                    scion_links.push(ScionLinkInfo { r1, r2, link_type: ScionLinkType::Child });
                }
            }

            // Intermediate to leaf links
            for leaf_idx in 0..config.leaves_per_intermediate {
                let leaf_asn = 300 + int_idx * 100 + leaf_idx;
                let leaf = IsdAs::new(isd, leaf_asn as u32);
                let int_router_idx = leaf_idx % config.border_routers_per_intermediate;
                if let Some((r1, r2)) = get_link_routers(&as_routers, intermediate, leaf,
                    int_router_idx, 0) {
                    all_links.push((r1, r2));
                    scion_links.push(ScionLinkInfo { r1, r2, link_type: ScionLinkType::Child });
                }
            }
        }
    }

    // Inter-ISD core links (sparse mesh)
    for i in 0..config.num_isds {
        let j = (i + 1) % config.num_isds;
        let isd1 = (i + 1) as u16;
        let isd2 = (j + 1) as u16;
        let core1 = IsdAs::new(isd1, 100);
        let core2 = IsdAs::new(isd2, 100);
        if let Some((r1, r2)) = get_link_routers(&as_routers, core1, core2, 1, 1) {
            all_links.push((r1, r2));
            scion_links.push(ScionLinkInfo { r1, r2, link_type: ScionLinkType::Core });
        }
    }

    // Add ALL links in one bulk operation
    net.add_links_from(all_links).unwrap();

    // Add SCION links (these are just metadata, already efficient)
    for link in scion_links {
        let _ = net.add_scion_link(link.r1, link.r2, link.link_type);
    }

    let mut all_ases = Vec::with_capacity(core_ases.len() + intermediate_ases.len() + leaf_ases.len());
    all_ases.extend(&core_ases);
    all_ases.extend(&intermediate_ases);
    all_ases.extend(&leaf_ases);

    let add_links_time = add_links_start.elapsed();

    let topology = ScalableTopology {
        core_ases,
        intermediate_ases,
        leaf_ases,
        all_ases,
        as_routers,
    };

    (net, topology, create_as_time, add_links_time)
}

/// Get link routers between two ASes
fn get_link_routers(
    as_routers: &HashMap<IsdAs, Vec<RouterId>>,
    as1: IsdAs,
    as2: IsdAs,
    router1_idx: usize,
    router2_idx: usize,
) -> Option<(RouterId, RouterId)> {
    let routers1 = as_routers.get(&as1)?;
    let routers2 = as_routers.get(&as2)?;
    Some((
        routers1[router1_idx % routers1.len()],
        routers2[router2_idx % routers2.len()],
    ))
}

/// Create an AS with specified number of border routers
/// Returns routers and internal links (deferred for bulk creation)
fn create_as(
    net: &mut ScionNetwork,
    isd_as: IsdAs,
    num_border_routers: usize,
    is_core: bool,
) -> (Vec<RouterId>, Vec<(RouterId, RouterId)>) {
    let mut routers = Vec::with_capacity(num_border_routers);
    let mut internal_links = Vec::new();

    for i in 0..num_border_routers {
        let name = format!("{}_{}", isd_as, i);
        let asn = isd_as.asn.0;
        let router = net.add_router(&name, asn);
        routers.push(router);
    }

    for &router in &routers {
        net.enable_scion_router(router, isd_as, is_core).unwrap();
    }

    // Collect internal full mesh links (don't add yet)
    for i in 0..routers.len() {
        for j in (i + 1)..routers.len() {
            internal_links.push((routers[i], routers[j]));
        }
    }

    (routers, internal_links)
}

// ============================================================================
// Scalability Tests
// ============================================================================

#[test]
#[ignore] // Run with: cargo test test_scale_tiny -- --nocapture --ignored
fn test_scale_tiny() {
    let config = ScaleConfig {
        num_isds: 3,
        cores_per_isd: 2,
        intermediates_per_isd: 2,
        leaves_per_intermediate: 3,
        border_routers_per_core: 4,
        border_routers_per_intermediate: 2,
        border_routers_per_leaf: 1,
    };
    // 3 ISDs × (2 + 2 + 6) = 30 ASes
    // Routers: 3 × (8 + 4 + 6) = 54

    println!("Running TINY scale test ({} ASes, {} routers)...",
             config.total_ases(), config.total_routers());
    let results = run_scale_test(config);
    results.print(&ScaleConfig {
        num_isds: 3,
        cores_per_isd: 2,
        intermediates_per_isd: 2,
        leaves_per_intermediate: 3,
        border_routers_per_core: 4,
        border_routers_per_intermediate: 2,
        border_routers_per_leaf: 1,
    });
}

#[test]
#[ignore]
fn test_scale_small() {
    let config = ScaleConfig {
        num_isds: 5,
        cores_per_isd: 3,
        intermediates_per_isd: 5,
        leaves_per_intermediate: 5,
        border_routers_per_core: 4,
        border_routers_per_intermediate: 2,
        border_routers_per_leaf: 1,
    };
    // 5 ISDs × (3 + 5 + 25) = 165 ASes
    // Routers: 5 × (12 + 10 + 25) = 235

    println!("Running SMALL scale test ({} ASes, {} routers)...",
             config.total_ases(), config.total_routers());
    let results = run_scale_test(config);
    results.print(&ScaleConfig {
        num_isds: 5,
        cores_per_isd: 3,
        intermediates_per_isd: 5,
        leaves_per_intermediate: 5,
        border_routers_per_core: 4,
        border_routers_per_intermediate: 2,
        border_routers_per_leaf: 1,
    });
}

#[test]
#[ignore]
fn test_scale_medium() {
    let config = ScaleConfig {
        num_isds: 10,
        cores_per_isd: 4,
        intermediates_per_isd: 10,
        leaves_per_intermediate: 8,
        border_routers_per_core: 6,
        border_routers_per_intermediate: 3,
        border_routers_per_leaf: 1,
    };
    // 10 ISDs × (4 + 10 + 80) = 940 ASes
    // Routers: 10 × (24 + 30 + 80) = 1,340

    println!("Running MEDIUM scale test ({} ASes, {} routers)...",
             config.total_ases(), config.total_routers());
    let results = run_scale_test(config);
    results.print(&ScaleConfig {
        num_isds: 10,
        cores_per_isd: 4,
        intermediates_per_isd: 10,
        leaves_per_intermediate: 8,
        border_routers_per_core: 6,
        border_routers_per_intermediate: 3,
        border_routers_per_leaf: 1,
    });
}

#[test]
#[ignore]
fn test_scale_large() {
    let config = ScaleConfig {
        num_isds: 20,
        cores_per_isd: 5,
        intermediates_per_isd: 15,
        leaves_per_intermediate: 10,
        border_routers_per_core: 8,
        border_routers_per_intermediate: 4,
        border_routers_per_leaf: 2,
    };
    // 20 ISDs × (5 + 15 + 150) = 3,400 ASes
    // Routers: 20 × (40 + 60 + 300) = 8,000

    println!("Running LARGE scale test ({} ASes, {} routers)...",
             config.total_ases(), config.total_routers());
    let results = run_scale_test(config);
    results.print(&ScaleConfig {
        num_isds: 20,
        cores_per_isd: 5,
        intermediates_per_isd: 15,
        leaves_per_intermediate: 10,
        border_routers_per_core: 8,
        border_routers_per_intermediate: 4,
        border_routers_per_leaf: 2,
    });
}

#[test]
#[ignore]
fn test_scale_xlarge() {
    let config = ScaleConfig {
        num_isds: 50,
        cores_per_isd: 6,
        intermediates_per_isd: 20,
        leaves_per_intermediate: 15,
        border_routers_per_core: 10,
        border_routers_per_intermediate: 4,
        border_routers_per_leaf: 2,
    };
    // 50 ISDs × (6 + 20 + 300) = 16,300 ASes
    // Routers: 50 × (60 + 80 + 600) = 37,000

    println!("Running XLARGE scale test ({} ASes, {} routers)...",
             config.total_ases(), config.total_routers());
    let results = run_scale_test(config);
    results.print(&ScaleConfig {
        num_isds: 50,
        cores_per_isd: 6,
        intermediates_per_isd: 20,
        leaves_per_intermediate: 15,
        border_routers_per_core: 10,
        border_routers_per_intermediate: 4,
        border_routers_per_leaf: 2,
    });
}

/// Run all scales and create a summary comparison table
#[test]
#[ignore]
fn test_scale_comparison() {
    println!("\n=== SCION Scalability Comparison ===\n");

    let configs = vec![
        ("TINY", ScaleConfig {
            num_isds: 3,
            cores_per_isd: 2,
            intermediates_per_isd: 2,
            leaves_per_intermediate: 3,
            border_routers_per_core: 4,
            border_routers_per_intermediate: 2,
            border_routers_per_leaf: 1,
        }),
        ("SMALL", ScaleConfig {
            num_isds: 5,
            cores_per_isd: 3,
            intermediates_per_isd: 5,
            leaves_per_intermediate: 5,
            border_routers_per_core: 4,
            border_routers_per_intermediate: 2,
            border_routers_per_leaf: 1,
        }),
        ("MEDIUM", ScaleConfig {
            num_isds: 10,
            cores_per_isd: 4,
            intermediates_per_isd: 10,
            leaves_per_intermediate: 8,
            border_routers_per_core: 6,
            border_routers_per_intermediate: 3,
            border_routers_per_leaf: 1,
        }),
        ("LARGE", ScaleConfig {
            num_isds: 20,
            cores_per_isd: 5,
            intermediates_per_isd: 15,
            leaves_per_intermediate: 10,
            border_routers_per_core: 8,
            border_routers_per_intermediate: 4,
            border_routers_per_leaf: 2,
        }),
    ];

    println!("{:<10} {:>8} {:>10} {:>12} {:>12} {:>12} {:>12} {:>12}",
             "Scale", "ASes", "Routers", "Topology", "CoreBcn", "IntraISD", "SegReg", "Total");
    println!("{}", "-".repeat(100));

    for (name, config) in configs {
        let ases = config.total_ases();
        let routers = config.total_routers();
        let results = run_scale_test(config);

        println!("{:<10} {:>8} {:>10} {:>10.1}ms {:>10.1}ms {:>10.1}ms {:>10.1}ms {:>10.1}ms",
                 name, ases, routers,
                 results.topology_build.as_secs_f64() * 1000.0,
                 results.core_beaconing.as_secs_f64() * 1000.0,
                 results.intra_isd_beaconing.as_secs_f64() * 1000.0,
                 results.segment_registration.as_secs_f64() * 1000.0,
                 results.total.as_secs_f64() * 1000.0);
    }
}
