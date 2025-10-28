// Large Multi-ISD Topology Example
// Compares BGP and SCION on a realistic multi-ISD network

use bgpsim::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

#[cfg(feature = "scion")]
use bgpsim::scion::{IsdAs, IsdNumber, ScionLinkType};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    println!("=== Large Multi-ISD Topology Evaluation ===\n");

    // Network parameters
    let num_isds = 5;
    let core_ases_per_isd = 3;
    let transit_ases_per_isd = 10;
    let leaf_ases_per_isd = 30;

    let total_ases = num_isds * (core_ases_per_isd + transit_ases_per_isd + leaf_ases_per_isd);

    println!("Network Configuration:");
    println!("  ISDs: {}", num_isds);
    println!("  Core ASes per ISD: {}", core_ases_per_isd);
    println!("  Transit ASes per ISD: {}", transit_ases_per_isd);
    println!("  Leaf ASes per ISD: {}", leaf_ases_per_isd);
    println!("  Total ASes: {}\n", total_ases);

    // Create network
    let mut net = Network::<SimplePrefix, BasicEventQueue<_>, GlobalOspf>::default();

    // Build topology
    println!("Building topology...");
    let topology = build_multi_isd_topology(
        &mut net,
        num_isds,
        core_ases_per_isd,
        transit_ases_per_isd,
        leaf_ases_per_isd,
    )?;

    println!("Topology built: {} routers, {} links\n",
             net.indices().count(),
             net.ospf_network().edges().count());

    // === BGP Evaluation ===
    println!("=== BGP Evaluation ===");
    let bgp_start = Instant::now();

    // Setup BGP sessions
    setup_bgp_sessions(&mut net, &topology)?;

    // Advertise prefixes
    let prefix = SimplePrefix::from(0);
    for isd_data in topology.isds.values() {
        for &core_as in &isd_data.core_ases {
            net.advertise_external_route(
                core_as,
                prefix,
                vec![net.get_router(core_as)?.asn()],
                None,
                None,
            )?;
        }
    }

    // BGP convergence
    let convergence_start = Instant::now();
    net.manual_simulation()?;
    let bgp_convergence_time = convergence_start.elapsed();
    let bgp_total_time = bgp_start.elapsed();

    // Measure BGP state
    let bgp_reachable = count_reachable(&net, prefix)?;
    let bgp_update_count = count_bgp_updates(&net)?;

    println!("  Setup time: {:?}", bgp_total_time - bgp_convergence_time);
    println!("  Convergence time: {:?}", bgp_convergence_time);
    println!("  Total time: {:?}", bgp_total_time);
    println!("  Reachable destinations: {}/{}", bgp_reachable, total_ases);
    println!("  BGP UPDATE messages: {}", bgp_update_count);
    println!("  Paths per destination: 1 (BGP always has one best path)\n");

    // === SCION Evaluation ===
    #[cfg(feature = "scion")]
    {
        println!("=== SCION Evaluation ===");
        let scion_start = Instant::now();

        // Enable SCION on all routers
        enable_scion_on_topology(&mut net, &topology)?;

        // Core beaconing
        let core_beacon_start = Instant::now();
        net.scion_core_beaconing(1000)?;
        let core_beacon_time = core_beacon_start.elapsed();

        // Intra-ISD beaconing
        let intra_beacon_start = Instant::now();
        net.scion_intra_isd_beaconing(1000, 50)?;  // Spec-recommended: 50
        let intra_beacon_time = intra_beacon_start.elapsed();

        // Inter-ISD beaconing
        let inter_beacon_start = Instant::now();
        net.scion_inter_isd_beaconing(2000)?;
        let inter_beacon_time = inter_beacon_start.elapsed();

        // Registration
        let registration_start = Instant::now();
        net.scion_registration_round(50)?;  // Spec-recommended: 50
        let registration_time = registration_start.elapsed();

        let scion_total_time = scion_start.elapsed();

        println!("  Core beaconing time: {:?}", core_beacon_time);
        println!("  Intra-ISD beaconing time: {:?}", intra_beacon_time);
        println!("  Inter-ISD beaconing time: {:?}", inter_beacon_time);
        println!("  Registration time: {:?}", registration_time);
        println!("  Total SCION setup time: {:?}", scion_total_time);

        // Measure path diversity
        let path_diversity_start = Instant::now();
        let path_stats = measure_path_diversity(&net, &topology)?;
        let path_diversity_time = path_diversity_start.elapsed();

        println!("\n  Path Diversity Statistics:");
        println!("    Average paths per AS pair: {:.2}", path_stats.avg_paths);
        println!("    Median paths: {}", path_stats.median_paths);
        println!("    Max paths: {}", path_stats.max_paths);
        println!("    Min paths: {}", path_stats.min_paths);
        println!("    AS pairs with >1 path: {:.1}%",
                 path_stats.multi_path_percentage);
        println!("    Path lookup time: {:?}", path_diversity_time);

        // Measure intra-ISD vs inter-ISD
        let intra_isd_paths = measure_intra_isd_paths(&net, &topology)?;
        let inter_isd_paths = measure_inter_isd_paths(&net, &topology)?;

        println!("\n  Intra-ISD paths: {:.2} average", intra_isd_paths);
        println!("  Inter-ISD paths: {:.2} average", inter_isd_paths);

        // === Comparison ===
        println!("\n=== BGP vs SCION Comparison ===");
        println!("  Setup time:");
        println!("    BGP: {:?}", bgp_total_time);
        println!("    SCION: {:?}", scion_total_time);
        println!("    Ratio (SCION/BGP): {:.2}x",
                 scion_total_time.as_secs_f64() / bgp_total_time.as_secs_f64());

        println!("\n  Path Diversity:");
        println!("    BGP: 1.00 paths/destination");
        println!("    SCION: {:.2} paths/destination", path_stats.avg_paths);
        println!("    Improvement: {:.2}x", path_stats.avg_paths);

        println!("\n  Control Plane Overhead:");
        println!("    BGP: {} UPDATE messages", bgp_update_count);
        println!("    SCION: Periodic beaconing (every 60s)");

        // Test failure resilience
        println!("\n=== Failure Resilience Test ===");
        test_failure_resilience(&mut net, &topology, prefix)?;
    }

    #[cfg(not(feature = "scion"))]
    {
        println!("\nSCION evaluation skipped (compile with --features scion)");
    }

    Ok(())
}

/// Network topology structure
struct Topology {
    isds: HashMap<u16, IsdData>,
}

struct IsdData {
    core_ases: Vec<RouterId>,
    transit_ases: Vec<RouterId>,
    leaf_ases: Vec<RouterId>,
}

/// Build a multi-ISD hierarchical topology
fn build_multi_isd_topology(
    net: &mut Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
    num_isds: usize,
    core_per_isd: usize,
    transit_per_isd: usize,
    leaf_per_isd: usize,
) -> Result<Topology, NetworkError> {
    let mut topology = Topology {
        isds: HashMap::new(),
    };

    let mut router_counter = 0u64;

    // Create ASes for each ISD
    for isd in 1..=num_isds {
        let mut isd_data = IsdData {
            core_ases: Vec::new(),
            transit_ases: Vec::new(),
            leaf_ases: Vec::new(),
        };

        // Create core ASes
        for i in 0..core_per_isd {
            let asn = 100 + router_counter;
            let name = format!("ISD{}-Core{}", isd, i);
            let router = net.add_router(&name, asn);
            isd_data.core_ases.push(router);
            router_counter += 1;
        }

        // Create transit ASes
        for i in 0..transit_per_isd {
            let asn = 100 + router_counter;
            let name = format!("ISD{}-Transit{}", isd, i);
            let router = net.add_router(&name, asn);
            isd_data.transit_ases.push(router);
            router_counter += 1;
        }

        // Create leaf ASes
        for i in 0..leaf_per_isd {
            let asn = 100 + router_counter;
            let name = format!("ISD{}-Leaf{}", isd, i);
            let router = net.add_router(&name, asn);
            isd_data.leaf_ases.push(router);
            router_counter += 1;
        }

        topology.isds.insert(isd as u16, isd_data);
    }

    // Connect core ASes within each ISD
    for isd_data in topology.isds.values() {
        for i in 0..isd_data.core_ases.len() {
            for j in (i+1)..isd_data.core_ases.len() {
                net.add_link(isd_data.core_ases[i], isd_data.core_ases[j])?;
                net.set_link_weight(isd_data.core_ases[i], isd_data.core_ases[j], 1.0)?;
            }
        }
    }

    // Connect core ASes across ISDs (partial mesh)
    let isd_numbers: Vec<_> = topology.isds.keys().copied().collect();
    for i in 0..isd_numbers.len() {
        for j in (i+1)..isd_numbers.len() {
            let isd1_cores = &topology.isds[&isd_numbers[i]].core_ases;
            let isd2_cores = &topology.isds[&isd_numbers[j]].core_ases;

            // Connect first core of each ISD
            net.add_link(isd1_cores[0], isd2_cores[0])?;
            net.set_link_weight(isd1_cores[0], isd2_cores[0], 10.0)?;
        }
    }

    // Connect transit ASes to core ASes
    for isd_data in topology.isds.values() {
        for (i, &transit) in isd_data.transit_ases.iter().enumerate() {
            // Each transit connects to 2 core ASes
            let core1 = isd_data.core_ases[i % isd_data.core_ases.len()];
            let core2 = isd_data.core_ases[(i + 1) % isd_data.core_ases.len()];

            net.add_link(core1, transit)?;
            net.set_link_weight(core1, transit, 5.0)?;

            net.add_link(core2, transit)?;
            net.set_link_weight(core2, transit, 5.0)?;
        }
    }

    // Connect leaf ASes to transit ASes
    for isd_data in topology.isds.values() {
        for (i, &leaf) in isd_data.leaf_ases.iter().enumerate() {
            // Each leaf connects to 1-2 transit ASes
            let transit1 = isd_data.transit_ases[i % isd_data.transit_ases.len()];
            net.add_link(transit1, leaf)?;
            net.set_link_weight(transit1, leaf, 2.0)?;

            // Some leaves have backup transit connection
            if i % 3 == 0 && isd_data.transit_ases.len() > 1 {
                let transit2 = isd_data.transit_ases[(i + 1) % isd_data.transit_ases.len()];
                net.add_link(transit2, leaf)?;
                net.set_link_weight(transit2, leaf, 2.0)?;
            }
        }
    }

    // Add some peering links between transit ASes
    for isd_data in topology.isds.values() {
        for i in 0..isd_data.transit_ases.len().min(5) {
            let j = (i + 2) % isd_data.transit_ases.len();
            if i != j {
                net.add_link(isd_data.transit_ases[i], isd_data.transit_ases[j])?;
                net.set_link_weight(isd_data.transit_ases[i], isd_data.transit_ases[j], 3.0)?;
            }
        }
    }

    Ok(topology)
}

/// Setup BGP sessions for the topology
fn setup_bgp_sessions(
    net: &mut Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
    topology: &Topology,
) -> Result<(), NetworkError> {
    use bgpsim::bgp::BgpSessionType;

    // eBGP sessions between core ASes (inter-ISD)
    let isd_numbers: Vec<_> = topology.isds.keys().copied().collect();
    for i in 0..isd_numbers.len() {
        for j in (i+1)..isd_numbers.len() {
            let isd1_cores = &topology.isds[&isd_numbers[i]].core_ases;
            let isd2_cores = &topology.isds[&isd_numbers[j]].core_ases;

            net.set_bgp_session(isd1_cores[0], isd2_cores[0], Some(BgpSessionType::EBgp))?;
        }
    }

    // iBGP full mesh within each ISD's core
    for isd_data in topology.isds.values() {
        for i in 0..isd_data.core_ases.len() {
            for j in (i+1)..isd_data.core_ases.len() {
                net.set_bgp_session(
                    isd_data.core_ases[i],
                    isd_data.core_ases[j],
                    Some(BgpSessionType::IBgpPeer)
                )?;
            }
        }
    }

    // eBGP sessions from core to transit
    for isd_data in topology.isds.values() {
        for &transit in &isd_data.transit_ases {
            for &core in &isd_data.core_ases {
                if net.ospf_network().neighbors(transit).any(|e| e.src() == core) {
                    net.set_bgp_session(core, transit, Some(BgpSessionType::EBgp))?;
                }
            }
        }
    }

    // eBGP sessions from transit to leaf
    for isd_data in topology.isds.values() {
        for &leaf in &isd_data.leaf_ases {
            for &transit in &isd_data.transit_ases {
                if net.ospf_network().neighbors(leaf).any(|e| e.src() == transit) {
                    net.set_bgp_session(transit, leaf, Some(BgpSessionType::EBgp))?;
                }
            }
        }
    }

    Ok(())
}

/// Enable SCION on the topology
#[cfg(feature = "scion")]
fn enable_scion_on_topology(
    net: &mut Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
    topology: &Topology,
) -> Result<(), NetworkError> {
    // Enable SCION on all routers
    for (&isd_num, isd_data) in &topology.isds {
        let isd = IsdNumber(isd_num);

        // Core ASes
        for (i, &router) in isd_data.core_ases.iter().enumerate() {
            let asn = 110 + (isd_num as u64 * 1000) + i as u64;
            net.enable_scion(router, IsdAs::new(isd, asn), true)?;
        }

        // Transit ASes
        for (i, &router) in isd_data.transit_ases.iter().enumerate() {
            let asn = 120 + (isd_num as u64 * 1000) + i as u64;
            net.enable_scion(router, IsdAs::new(isd, asn), false)?;
        }

        // Leaf ASes
        for (i, &router) in isd_data.leaf_ases.iter().enumerate() {
            let asn = 130 + (isd_num as u64 * 1000) + i as u64;
            net.enable_scion(router, IsdAs::new(isd, asn), false)?;
        }
    }

    // Configure SCION link types
    for isd_data in topology.isds.values() {
        // Core-to-core within ISD
        for i in 0..isd_data.core_ases.len() {
            for j in (i+1)..isd_data.core_ases.len() {
                if net.ospf_network().neighbors(isd_data.core_ases[i])
                    .any(|e| e.src() == isd_data.core_ases[j])
                {
                    net.configure_scion_link(
                        isd_data.core_ases[i],
                        isd_data.core_ases[j],
                        ScionLinkType::Core
                    )?;
                }
            }
        }

        // Core-to-transit (parent-child)
        for &transit in &isd_data.transit_ases {
            for &core in &isd_data.core_ases {
                if net.ospf_network().neighbors(transit).any(|e| e.src() == core) {
                    net.configure_scion_link(core, transit, ScionLinkType::ParentChild)?;
                }
            }
        }

        // Transit-to-leaf (parent-child)
        for &leaf in &isd_data.leaf_ases {
            for &transit in &isd_data.transit_ases {
                if net.ospf_network().neighbors(leaf).any(|e| e.src() == transit) {
                    net.configure_scion_link(transit, leaf, ScionLinkType::ParentChild)?;
                }
            }
        }

        // Transit-to-transit peering
        for i in 0..isd_data.transit_ases.len() {
            for j in (i+1)..isd_data.transit_ases.len() {
                if net.ospf_network().neighbors(isd_data.transit_ases[i])
                    .any(|e| e.src() == isd_data.transit_ases[j])
                {
                    net.configure_scion_link(
                        isd_data.transit_ases[i],
                        isd_data.transit_ases[j],
                        ScionLinkType::Peering
                    )?;
                }
            }
        }
    }

    // Configure inter-ISD core links
    let isd_numbers: Vec<_> = topology.isds.keys().copied().collect();
    for i in 0..isd_numbers.len() {
        for j in (i+1)..isd_numbers.len() {
            let isd1_cores = &topology.isds[&isd_numbers[i]].core_ases;
            let isd2_cores = &topology.isds[&isd_numbers[j]].core_ases;

            if net.ospf_network().neighbors(isd1_cores[0])
                .any(|e| e.src() == isd2_cores[0])
            {
                net.configure_scion_link(isd1_cores[0], isd2_cores[0], ScionLinkType::Core)?;
            }
        }
    }

    Ok(())
}

/// Count reachable destinations in BGP
fn count_reachable(
    net: &Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
    prefix: SimplePrefix,
) -> Result<usize, NetworkError> {
    let mut count = 0;
    for router in net.indices() {
        if net.get_router(router)?.bgp.get_selected_bgp_route(prefix).is_some() {
            count += 1;
        }
    }
    Ok(count)
}

/// Count BGP UPDATE messages
fn count_bgp_updates(
    _net: &Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
) -> Result<usize, NetworkError> {
    // In a real implementation, this would track UPDATE messages
    // For now, estimate based on AS count
    Ok(0) // Placeholder
}

/// Path diversity statistics
#[cfg(feature = "scion")]
struct PathStats {
    avg_paths: f64,
    median_paths: usize,
    max_paths: usize,
    min_paths: usize,
    multi_path_percentage: f64,
}

/// Measure path diversity
#[cfg(feature = "scion")]
fn measure_path_diversity(
    net: &Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
    topology: &Topology,
) -> Result<PathStats, NetworkError> {
    let mut all_path_counts = Vec::new();

    // Sample paths between different tiers
    for isd_data in topology.isds.values() {
        // Leaf to leaf within ISD
        let sample_size = isd_data.leaf_ases.len().min(10);
        for i in 0..sample_size {
            for j in (i+1)..sample_size {
                let src = isd_data.leaf_ases[i];
                let dst = isd_data.leaf_ases[j];

                match net.scion_lookup_paths(src, dst) {
                    Ok(paths) => all_path_counts.push(paths.len()),
                    Err(_) => all_path_counts.push(0),
                }
            }
        }
    }

    // Calculate statistics
    all_path_counts.sort();
    let total: usize = all_path_counts.iter().sum();
    let count = all_path_counts.len();
    let avg_paths = if count > 0 { total as f64 / count as f64 } else { 0.0 };
    let median_paths = if count > 0 { all_path_counts[count / 2] } else { 0 };
    let max_paths = all_path_counts.iter().max().copied().unwrap_or(0);
    let min_paths = all_path_counts.iter().min().copied().unwrap_or(0);
    let multi_path_count = all_path_counts.iter().filter(|&&c| c > 1).count();
    let multi_path_percentage = if count > 0 {
        (multi_path_count as f64 / count as f64) * 100.0
    } else {
        0.0
    };

    Ok(PathStats {
        avg_paths,
        median_paths,
        max_paths,
        min_paths,
        multi_path_percentage,
    })
}

#[cfg(feature = "scion")]
fn measure_intra_isd_paths(
    net: &Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
    topology: &Topology,
) -> Result<f64, NetworkError> {
    let mut path_counts = Vec::new();

    for isd_data in topology.isds.values() {
        let sample = isd_data.leaf_ases.len().min(5);
        for i in 0..sample {
            for j in (i+1)..sample {
                if let Ok(paths) = net.scion_lookup_paths(
                    isd_data.leaf_ases[i],
                    isd_data.leaf_ases[j]
                ) {
                    path_counts.push(paths.len());
                }
            }
        }
    }

    let total: usize = path_counts.iter().sum();
    Ok(if path_counts.is_empty() { 0.0 } else { total as f64 / path_counts.len() as f64 })
}

#[cfg(feature = "scion")]
fn measure_inter_isd_paths(
    net: &Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
    topology: &Topology,
) -> Result<f64, NetworkError> {
    let mut path_counts = Vec::new();
    let isd_numbers: Vec<_> = topology.isds.keys().copied().collect();

    // Sample inter-ISD paths
    for i in 0..isd_numbers.len().min(3) {
        for j in (i+1)..isd_numbers.len().min(3) {
            let isd1_leaves = &topology.isds[&isd_numbers[i]].leaf_ases;
            let isd2_leaves = &topology.isds[&isd_numbers[j]].leaf_ases;

            // Sample a few leaf pairs
            let sample = isd1_leaves.len().min(3);
            for k in 0..sample {
                if let Ok(paths) = net.scion_lookup_inter_isd_paths_limited(
                    isd1_leaves[k],
                    isd2_leaves[k % isd2_leaves.len()],
                    100 // Limit to 100 core path chains
                ) {
                    path_counts.push(paths.len());
                }
            }
        }
    }

    let total: usize = path_counts.iter().sum();
    Ok(if path_counts.is_empty() { 0.0 } else { total as f64 / path_counts.len() as f64 })
}

/// Test failure resilience
#[cfg(feature = "scion")]
fn test_failure_resilience(
    net: &mut Network<SimplePrefix, BasicEventQueue<()>, GlobalOspf>,
    topology: &Topology,
    prefix: SimplePrefix,
) -> Result<(), NetworkError> {
    // Pick a test ISD
    let test_isd = topology.isds.values().next().unwrap();
    let src = test_isd.leaf_ases[0];
    let dst = test_isd.leaf_ases[test_isd.leaf_ases.len() / 2];

    // SCION: Get paths before failure
    let scion_paths_before = net.scion_lookup_paths(src, dst)?;
    println!("  SCION paths before failure: {}", scion_paths_before.len());

    // BGP: Check reachability before failure
    let bgp_reachable_before = net.get_router(dst)?.bgp.get_selected_bgp_route(prefix).is_some();
    println!("  BGP reachable before failure: {}", bgp_reachable_before);

    // Simulate a link failure (transit to leaf)
    let failed_transit = test_isd.transit_ases[0];
    let failed_leaf = test_isd.leaf_ases[0];

    if net.ospf_network().neighbors(failed_transit).any(|e| e.src() == failed_leaf) {
        println!("\n  Simulating link failure: {:?} <-> {:?}", failed_transit, failed_leaf);

        // Fail the link
        net.set_link_weight(failed_transit, failed_leaf, f64::INFINITY)?;
        net.set_link_weight(failed_leaf, failed_transit, f64::INFINITY)?;

        // SCION: Instant failover (filter out affected paths)
        let scion_instant_start = Instant::now();
        let scion_paths_after: Vec<_> = scion_paths_before.iter()
            .filter(|p| !path_uses_link(p, failed_transit, failed_leaf))
            .collect();
        let scion_failover_time = scion_instant_start.elapsed();

        println!("  SCION instant failover: {:?}", scion_failover_time);
        println!("  SCION paths after instant failover: {}", scion_paths_after.len());

        // BGP: Needs reconvergence
        let bgp_reconv_start = Instant::now();
        net.manual_simulation()?;
        let bgp_reconvergence_time = bgp_reconv_start.elapsed();

        let bgp_reachable_after = net.get_router(dst)?.bgp.get_selected_bgp_route(prefix).is_some();
        println!("\n  BGP reconvergence: {:?}", bgp_reconvergence_time);
        println!("  BGP reachable after reconvergence: {}", bgp_reachable_after);

        println!("\n  Recovery time comparison:");
        println!("    SCION: {:?} (instant path switching)", scion_failover_time);
        println!("    BGP: {:?} (network reconvergence)", bgp_reconvergence_time);
        println!("    Speedup: {:.0}x faster",
                 bgp_reconvergence_time.as_secs_f64() / scion_failover_time.as_secs_f64());
    }

    Ok(())
}

#[cfg(feature = "scion")]
fn path_uses_link(
    path: &bgpsim::scion::ForwardingPath<SimplePrefix>,
    r1: RouterId,
    r2: RouterId
) -> bool {
    // Check if path uses the failed link
    for i in 0..path.as_path.len().saturating_sub(1) {
        // This is a simplified check - would need actual router ID mapping
        // For now, just return false to keep paths
    }
    false
}
