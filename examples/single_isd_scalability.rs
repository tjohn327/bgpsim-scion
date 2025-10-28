// SPDX-License-Identifier: GPL-2.0-or-later
//
// Single-ISD Scalability Measurement
//
// Measures scalability for SINGLE-ISD topologies (simpler than multi-ISD)
// Uses SCION specification defaults (draft-dekater-scion-controlplane-10):
//   - max_pcbs = 50 (recommended best PCBs set size)
//   - Topology-aware beaconing rounds based on hierarchy depth

use bgpsim::prelude::*;
use bgpsim::event::BasicEventQueue;
use bgpsim::types::{SimplePrefix, ASN};
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::{IsdAs, IsdNumber, ScionLinkType};
use std::time::Instant;

fn main() -> Result<(), NetworkError> {
    println!("=== SCION Single-ISD Scalability Measurement ===");
    println!("System: 24 cores, 32GB RAM");
    println!("Spec: draft-dekater-scion-controlplane-10");
    println!("  max_pcbs = 50 (spec-recommended)");
    println!("  topology-aware beaconing rounds\n");

    let sizes = vec![100, 1000, 10000];

    for size in sizes {
        measure_scale(size)?;
    }

    println!("=== Measurement Complete ===");
    Ok(())
}

fn measure_scale(size: usize) -> Result<(), NetworkError> {
    println!("============================================================");
    println!("Testing {} routers (single ISD)", size);
    println!("============================================================\n");

    // Calculate hierarchy
    let core_count = ((size as f64 * 0.05).ceil().max(2.0)) as usize;
    let transit_count = ((size as f64 * 0.25).ceil()) as usize;
    let leaf_count = size.saturating_sub(core_count + transit_count);

    println!("Topology: 1 ISD, {} core, {} transit, {} leaf",
        core_count, transit_count, leaf_count);

    // 1. Create topology
    let start = Instant::now();
    let mut net = create_single_isd_topology(size, core_count, transit_count, leaf_count)?;
    let topo_time = start.elapsed();
    println!("1. Topology creation: {:.3}s", topo_time.as_secs_f64());
    println!("   Created {} routers", net.indices().count());

    // 2. Enable SCION
    let start = Instant::now();
    enable_scion(&mut net, size, core_count)?;
    let scion_time = start.elapsed();
    println!("2. SCION enablement: {:.3}s", scion_time.as_secs_f64());

    // 3. Core Beaconing
    let start = Instant::now();
    let core_pcbs = net.scion_core_beaconing(1000)?;
    let core_beacon_time = start.elapsed();
    println!("3. Core beaconing: {:.3}s ({} PCBs)",
        core_beacon_time.as_secs_f64(), core_pcbs);

    // 4. Intra-ISD Beaconing
    let hierarchy_depth = estimate_hierarchy_depth(core_count, transit_count, leaf_count);
    let rounds = hierarchy_depth + 1;

    let start = Instant::now();
    for _ in 0..rounds {
        net.scion_intra_isd_beaconing(1000, 50)?;
    }
    let intra_beacon_time = start.elapsed();
    println!("4. Intra-ISD beaconing: {:.3}s ({} rounds, depth ~{})",
        intra_beacon_time.as_secs_f64(), rounds, hierarchy_depth);

    // 5. Registration
    let start = Instant::now();
    let (up, down, core) = net.scion_registration_round(50)?;
    let reg_time = start.elapsed();
    println!("5. Registration: {:.3}s", reg_time.as_secs_f64());
    println!("   Registered: {} up, {} down, {} core segments", up, down, core);

    // 6. Path Lookup Performance
    let routers: Vec<_> = net.indices().take(10.min(size)).collect();
    let start = Instant::now();
    let mut path_count = 0;
    for i in 0..routers.len() {
        for j in 0..routers.len() {
            if i != j {
                if let Ok(paths) = net.scion_lookup_paths(routers[i], routers[j]) {
                    path_count += paths.len();
                }
            }
        }
    }
    let lookup_time = start.elapsed();
    let queries = routers.len() * (routers.len() - 1);
    let avg_lookup = lookup_time.as_secs_f64() / queries as f64;
    println!("6. Path lookup (avg of {} queries): {:.6}s", queries, avg_lookup);
    println!("   Found {} total paths", path_count);

    let total_time = topo_time + scion_time + core_beacon_time + intra_beacon_time + reg_time;
    println!("\nTotal setup time: {:.3}s\n", total_time.as_secs_f64());

    Ok(())
}

fn create_single_isd_topology(
    size: usize,
    core_count: usize,
    transit_count: usize,
    leaf_count: usize,
) -> Result<Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>, NetworkError> {
    let mut net = Network::default();

    let mut cores = Vec::new();
    let mut transits = Vec::new();
    let mut leaves = Vec::new();

    // Create routers
    for i in 0..core_count {
        let asn = i as u64 + 100;
        let router = net.add_router(&format!("Core{}", i), ASN(asn as u32));
        cores.push(router);
    }

    for i in 0..transit_count {
        let asn = (core_count + i) as u64 + 100;
        let router = net.add_router(&format!("Transit{}", i), ASN(asn as u32));
        transits.push(router);
    }

    for i in 0..leaf_count {
        let asn = (core_count + transit_count + i) as u64 + 100;
        let router = net.add_router(&format!("Leaf{}", i), ASN(asn as u32));
        leaves.push(router);
    }

    // Core mesh
    for i in 0..cores.len() {
        for j in (i+1)..cores.len() {
            net.add_link(cores[i], cores[j])?;
        }
    }

    // Core to transit
    for (i, &transit) in transits.iter().enumerate() {
        let core = cores[i % cores.len()];
        net.add_link(core, transit)?;
    }

    // Transit to leaf
    for (i, &leaf) in leaves.iter().enumerate() {
        let transit = transits[i % transits.len()];
        net.add_link(transit, leaf)?;
    }

    Ok(net)
}

fn enable_scion(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    size: usize,
    core_count: usize,
) -> Result<(), NetworkError> {
    let all_routers: Vec<_> = net.indices().collect();

    // Enable SCION on all routers (all in ISD 1)
    for (idx, &router) in all_routers.iter().enumerate() {
        let asn = idx as u64 + 100;
        let is_core = idx < core_count;
        net.enable_scion(router, IsdAs::new(1u16, asn), is_core)?;
    }

    // Configure SCION links
    let routers_to_configure: Vec<_> = net.indices().collect();
    for &router in &routers_to_configure {
        let r_is_core = net.get_router(router)?.scion().unwrap().is_core;

        let neighbors: Vec<_> = net.ospf_network().neighbors(router)
            .map(|e| e.src())
            .collect();

        for neighbor in neighbors {
            // Only configure each link once
            if router.index() >= neighbor.index() {
                continue;
            }

            let n_is_core = net.get_router(neighbor)?.scion().unwrap().is_core;

            // Determine link type
            let link_type = if r_is_core && n_is_core {
                ScionLinkType::Core
            } else if r_is_core || n_is_core {
                ScionLinkType::ParentChild
            } else {
                ScionLinkType::Peering
            };

            let _ = net.configure_scion_link(router, neighbor, link_type);
        }
    }

    Ok(())
}

fn estimate_hierarchy_depth(core_count: usize, transit_count: usize, leaf_count: usize) -> usize {
    let mut depth: usize = 0;
    if core_count > 0 { depth += 1; }
    if transit_count > 0 { depth += 1; }
    if leaf_count > 0 { depth += 1; }
    depth.saturating_sub(1).max(2)
}
