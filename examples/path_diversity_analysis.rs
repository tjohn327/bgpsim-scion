// SPDX-License-Identifier: GPL-2.0-or-later
//
// Path Diversity Analysis
//
// Find optimal max_pcbs value for good path diversity vs performance tradeoff

use bgpsim::event::BasicEventQueue;
use bgpsim::ospf::GlobalOspf;
use bgpsim::prelude::*;
use bgpsim::scion::{IsdAs, IsdNumber, ScionLinkType};
use bgpsim::types::SimplePrefix;
use std::time::Instant;

fn main() -> Result<(), NetworkError> {
    println!("=== SCION Path Diversity Analysis ===");
    println!("Finding optimal max_pcbs for large topologies\n");

    // Test at 1K scale first (faster iteration)
    println!("Testing at 1000 routers:");
    analyze_path_diversity(1000)?;

    println!("\n{}\n", "=".repeat(70));

    // Test at 10K scale
    println!("Testing at 10000 routers:");
    analyze_path_diversity(10000)?;

    Ok(())
}

fn analyze_path_diversity(size: usize) -> Result<(), NetworkError> {
    // Test different max_pcbs values
    let max_pcbs_values = vec![5, 10, 20, 50, 100];

    println!(
        "\n{:<10} {:<15} {:<15} {:<15} {:<15}",
        "max_pcbs", "Beacon Time", "Reg Time", "Avg Paths", "Max Paths"
    );
    println!("{}", "-".repeat(70));

    for &max_pcbs in &max_pcbs_values {
        // Create fresh topology for each test
        let mut net = create_topology(size)?;
        enable_scion(&mut net, size)?;

        // Time beaconing
        let beacon_start = Instant::now();

        // Core beaconing creates initial PCBs (includes inter-ISD)
        net.scion_core_beaconing(1000)?;

        // Multiple rounds of intra-ISD beaconing to propagate through hierarchy
        // (depth of hierarchy depends on topology size)
        let rounds = if size >= 10000 {
            10
        } else if size >= 1000 {
            5
        } else {
            3
        };
        for _ in 0..rounds {
            net.scion_intra_isd_beaconing(1000, max_pcbs)?;
        }

        let beacon_time = beacon_start.elapsed();

        // Time registration
        let reg_start = Instant::now();
        let (up, down, core) = net.scion_registration_round(max_pcbs)?;
        let reg_time = reg_start.elapsed();

        // Sample path diversity (test 20 random pairs)
        let routers: Vec<_> = net.indices().collect();
        let sample_size = 20.min(routers.len());
        let mut path_counts = Vec::new();

        for i in 0..sample_size {
            for j in (i + 1)..sample_size {
                if let Ok(paths) = net.scion_lookup_paths(routers[i], routers[j]) {
                    path_counts.push(paths.len());
                }
            }
        }

        let avg_paths = if !path_counts.is_empty() {
            path_counts.iter().sum::<usize>() as f64 / path_counts.len() as f64
        } else {
            0.0
        };

        let max_paths = path_counts.iter().max().copied().unwrap_or(0);

        println!(
            "{:<10} {:<15.3} {:<15.3} {:<15.1} {:<15}",
            max_pcbs,
            beacon_time.as_secs_f64(),
            reg_time.as_secs_f64(),
            avg_paths,
            max_paths
        );

        // Show segment counts
        if max_pcbs == 5 || max_pcbs == 100 {
            println!("  └─ Segments: {} up, {} down, {} core", up, down, core);
        }
    }

    Ok(())
}

fn create_topology(
    size: usize,
) -> Result<Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>, NetworkError> {
    let mut net = Network::default();

    // Calculate topology parameters
    let num_isds = ((size as f64).sqrt().ceil() as usize).max(1);
    let ases_per_isd = size / num_isds;
    let core_per_isd = (ases_per_isd as f64 * 0.05).ceil().max(2.0) as usize;
    let transit_per_isd = (ases_per_isd as f64 * 0.25).ceil() as usize;
    let leaf_per_isd = ases_per_isd.saturating_sub(core_per_isd + transit_per_isd);

    let mut all_cores = Vec::new();

    // Create routers for each ISD
    for isd in 0..num_isds {
        let mut cores = Vec::new();
        let mut transits = Vec::new();
        let mut leaves = Vec::new();

        // Core ASes
        for i in 0..core_per_isd {
            let asn = (isd * ases_per_isd + i) as u64 + 100;
            let router = net.add_router(&format!("R{}", asn), ASN(asn as u32));
            cores.push(router);
        }

        // Transit ASes
        for i in 0..transit_per_isd {
            let asn = (isd * ases_per_isd + core_per_isd + i) as u64 + 100;
            let router = net.add_router(&format!("R{}", asn), ASN(asn as u32));
            transits.push(router);
        }

        // Leaf ASes
        for i in 0..leaf_per_isd {
            let asn = (isd * ases_per_isd + core_per_isd + transit_per_isd + i) as u64 + 100;
            let router = net.add_router(&format!("R{}", asn), ASN(asn as u32));
            leaves.push(router);
        }

        // Connect within ISD
        // Core mesh
        for i in 0..cores.len() {
            for j in (i + 1)..cores.len() {
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

        all_cores.push(cores);
    }

    // Inter-ISD core links
    for i in 0..all_cores.len() {
        let next_isd = (i + 1) % all_cores.len();
        if next_isd != i && !all_cores[i].is_empty() && !all_cores[next_isd].is_empty() {
            net.add_link(all_cores[i][0], all_cores[next_isd][0])?;
        }
    }

    Ok(net)
}

fn enable_scion(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    size: usize,
) -> Result<(), NetworkError> {
    let num_isds = ((size as f64).sqrt().ceil() as usize).max(1);
    let all_routers: Vec<_> = net.indices().collect();
    let ases_per_isd = all_routers.len() / num_isds;
    let core_per_isd = (ases_per_isd as f64 * 0.05).ceil().max(2.0) as usize;

    // Enable SCION on all routers
    let mut router_count = 0;
    for isd in 0..num_isds {
        for local_idx in 0..ases_per_isd {
            if router_count + local_idx >= all_routers.len() {
                break;
            }
            let router = all_routers[router_count + local_idx];
            let asn = (isd * ases_per_isd + local_idx) as u64 + 100;
            let is_core = local_idx < core_per_isd;
            let isd_num = IsdNumber((isd + 1) as u16);

            net.enable_scion(router, IsdAs::new(isd_num, asn), is_core)?;
        }
        router_count += ases_per_isd;
    }

    // Configure SCION links
    let routers_to_configure: Vec<_> = net.indices().collect();
    for &router in &routers_to_configure {
        let r_scion = match net.get_router(router).ok().and_then(|r| r.scion()) {
            Some(s) => s.isd_as,
            None => continue,
        };
        let r_is_core = net.get_router(router).unwrap().scion().unwrap().is_core;

        let neighbors: Vec<_> = net
            .ospf_network()
            .neighbors(router)
            .map(|e| e.src())
            .collect();

        for neighbor in neighbors {
            // Only configure each link once
            if router.index() >= neighbor.index() {
                continue;
            }

            let n_scion = match net.get_router(neighbor).ok().and_then(|r| r.scion()) {
                Some(s) => s.isd_as,
                None => continue,
            };
            let n_is_core = net.get_router(neighbor).unwrap().scion().unwrap().is_core;

            let link_type = if r_is_core && n_is_core {
                ScionLinkType::Core
            } else if r_is_core || n_is_core {
                ScionLinkType::ParentChild
            } else if r_scion.isd == n_scion.isd {
                ScionLinkType::Peering
            } else {
                continue;
            };

            let _ = net.configure_scion_link(router, neighbor, link_type);
        }
    }

    Ok(())
}
