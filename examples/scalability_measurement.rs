// SPDX-License-Identifier: GPL-2.0-or-later
//
// Scalability Measurement
//
// Manual timing of SCION operations at different scales
//
// Uses SCION specification defaults (draft-dekater-scion-controlplane-10):
//   - max_pcbs = 50 (recommended best PCBs set size)
//   - Topology-aware beaconing rounds based on hierarchy depth
//   - Registration limit = 50 segments

use bgpsim::prelude::*;
use bgpsim::event::BasicEventQueue;
use bgpsim::types::SimplePrefix;
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::{IsdAs, ScionLinkType};
use std::time::Instant;

struct IsdTopology {
    isd_number: u16,
    cores: Vec<RouterId>,
    transits: Vec<RouterId>,
    leaves: Vec<RouterId>,
}

fn main() -> Result<(), NetworkError> {
    println!("=== SCION Scalability Measurement ===");
    println!("System: 24 cores, 32GB RAM");
    println!("Spec: draft-dekater-scion-controlplane-10");
    println!("  max_pcbs = 50 (spec-recommended)");
    println!("  topology-aware beaconing rounds\n");

    // Test sizes
    let sizes = vec![10, 50, 100, 500, 1000];

    for size in sizes {
        println!("\n{}", "=".repeat(60));
        println!("Testing {} routers", size);
        println!("{}\n", "=".repeat(60));

        measure_scale(size)?;
    }

    println!("\n=== Measurement Complete ===");
    Ok(())
}

fn measure_scale(size: usize) -> Result<(), NetworkError> {
    // Calculate topology parameters
    let num_isds = ((size as f64).sqrt().ceil() as usize).max(1);
    let ases_per_isd = size / num_isds;
    let core_per_isd = (ases_per_isd as f64 * 0.05).ceil().max(2.0) as usize;
    let transit_per_isd = (ases_per_isd as f64 * 0.25).ceil() as usize;
    let leaf_per_isd = ases_per_isd.saturating_sub(core_per_isd + transit_per_isd);

    println!("Topology: {} ISDs, {} core/ISD, {} transit/ISD, {} leaf/ISD",
             num_isds, core_per_isd, transit_per_isd, leaf_per_isd);

    // 1. Topology Creation (creates routers, links, and enables SCION all together)
    let start = Instant::now();
    let mut net = create_and_configure_topology(num_isds, core_per_isd, transit_per_isd, leaf_per_isd)?;
    let topo_time = start.elapsed();

    let actual_routers = net.indices().count();
    println!("1. Topology creation & SCION enablement: {:.3}s", topo_time.as_secs_f64());
    println!("   Created {} routers", actual_routers);

    // Debug: Count SCION-enabled routers
    let mut scion_count = 0;
    let mut core_count = 0;
    for r in net.indices() {
        if let Ok(router) = net.get_router(r) {
            if let Some(scion) = router.scion() {
                scion_count += 1;
                if scion.is_core {
                    core_count += 1;
                }
            }
        }
    }
    println!("   SCION enabled: {} routers ({} core)", scion_count, core_count);

    // 2. Combined Beaconing (Core + Intra-ISD interleaved)
    let hierarchy_depth = estimate_hierarchy_depth(core_per_isd, transit_per_isd, leaf_per_isd);
    let intra_rounds_per_iter = hierarchy_depth + 1;

    let inter_isd_rounds = if num_isds > 1 {
        (num_isds / 2).max(2)
    } else {
        1
    };

    let start_beaconing = Instant::now();
    let mut total_core_pcbs = 0;
    let mut total_intra_pcbs = 0;
    let mut timestamp = 1000u32;

    for _round in 0..inter_isd_rounds {
        // Core beaconing
        let core_pcbs = net.scion_core_beaconing(timestamp)?;
        total_core_pcbs += core_pcbs;
        timestamp += 1;

        // Intra-ISD beaconing
        for _ in 0..intra_rounds_per_iter {
            let intra_pcbs = net.scion_intra_isd_beaconing(timestamp, 50)?;
            total_intra_pcbs += intra_pcbs;
            timestamp += 1;
        }
    }

    let beaconing_time = start_beaconing.elapsed();
    println!("2. Beaconing (interleaved): {:.3}s", beaconing_time.as_secs_f64());
    println!("   - Inter-ISD rounds: {} (for {} ISDs)", inter_isd_rounds, num_isds);
    println!("   - Intra-ISD rounds per iteration: {} (depth+1)", intra_rounds_per_iter);
    println!("   - Total core PCBs: {}", total_core_pcbs);
    println!("   - Total intra PCBs: {}", total_intra_pcbs);

    // 3. Registration
    let start = Instant::now();
    let (up, down, core) = net.scion_registration_round(50)?;
    let registration_time = start.elapsed();
    println!("3. Registration: {:.3}s", registration_time.as_secs_f64());
    println!("   Registered: {} up, {} down, {} core segments", up, down, core);

    // 4. Path Lookup
    let routers: Vec<_> = net.indices().take(10.min(actual_routers)).collect();
    let mut lookup_times = Vec::new();

    for i in 0..routers.len().min(5) {
        for j in (i+1)..routers.len().min(5) {
            let start = Instant::now();
            let paths = net.scion_lookup_paths(routers[i], routers[j])?;
            let lookup_time = start.elapsed();
            lookup_times.push(lookup_time);
        }
    }

    let avg_lookup = if !lookup_times.is_empty() {
        lookup_times.iter().sum::<std::time::Duration>() / lookup_times.len() as u32
    } else {
        std::time::Duration::from_secs(0)
    };
    println!("4. Path lookup (avg of {} queries): {:.6}s", lookup_times.len(), avg_lookup.as_secs_f64());

    // Total time
    let total = topo_time + beaconing_time + registration_time;
    println!("\nTotal setup time: {:.3}s", total.as_secs_f64());

    Ok(())
}

fn create_and_configure_topology(
    num_isds: usize,
    core_per_isd: usize,
    transit_per_isd: usize,
    leaf_per_isd: usize,
) -> Result<Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>, NetworkError> {
    let mut net = Network::default();
    let mut all_isds = Vec::new();

    // Create routers for each ISD and enable SCION immediately
    for isd_idx in 0..num_isds {
        let isd_number = (isd_idx + 1) as u16;  // ISDs are 1-based
        let mut isd_topo = IsdTopology {
            isd_number,
            cores: Vec::new(),
            transits: Vec::new(),
            leaves: Vec::new(),
        };

        // Core ASes
        for i in 0..core_per_isd {
            let asn = (isd_number as u64 * 100) + i as u64 + 10;
            let router = net.add_router(&format!("ISD{}-Core{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(router, IsdAs::new(isd_number, asn), true)?;  // Enable SCION immediately
            isd_topo.cores.push(router);
        }

        // Transit ASes
        for i in 0..transit_per_isd {
            let asn = (isd_number as u64 * 100) + (core_per_isd + i) as u64 + 10;
            let router = net.add_router(&format!("ISD{}-Transit{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(router, IsdAs::new(isd_number, asn), false)?;
            isd_topo.transits.push(router);
        }

        // Leaf ASes
        for i in 0..leaf_per_isd {
            let asn = (isd_number as u64 * 100) + (core_per_isd + transit_per_isd + i) as u64 + 10;
            let router = net.add_router(&format!("ISD{}-Leaf{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(router, IsdAs::new(isd_number, asn), false)?;
            isd_topo.leaves.push(router);
        }

        all_isds.push(isd_topo);
    }

    // Create links within each ISD and configure SCION links
    for isd_topo in &all_isds {
        // Core mesh
        for i in 0..isd_topo.cores.len() {
            for j in (i+1)..isd_topo.cores.len() {
                net.add_link(isd_topo.cores[i], isd_topo.cores[j])?;
                net.configure_scion_link(isd_topo.cores[i], isd_topo.cores[j], ScionLinkType::Core)?;
            }
        }

        // Core to transit
        for (i, &transit) in isd_topo.transits.iter().enumerate() {
            let core = isd_topo.cores[i % isd_topo.cores.len()];
            net.add_link(core, transit)?;
            net.configure_scion_link(core, transit, ScionLinkType::ParentChild)?;
        }

        // Transit to leaf
        for (i, &leaf) in isd_topo.leaves.iter().enumerate() {
            let transit = isd_topo.transits[i % isd_topo.transits.len()];
            net.add_link(transit, leaf)?;
            net.configure_scion_link(transit, leaf, ScionLinkType::ParentChild)?;
        }
    }

    // Inter-ISD core links (ring topology)
    for i in 0..all_isds.len() {
        let next = (i + 1) % all_isds.len();
        if next != i && !all_isds[i].cores.is_empty() && !all_isds[next].cores.is_empty() {
            net.add_link(all_isds[i].cores[0], all_isds[next].cores[0])?;
            net.configure_scion_link(all_isds[i].cores[0], all_isds[next].cores[0], ScionLinkType::Core)?;
        }
    }

    Ok(net)
}

fn estimate_hierarchy_depth(core_count: usize, transit_count: usize, leaf_count: usize) -> usize {
    let mut depth: usize = 0;
    if core_count > 0 { depth += 1; }
    if transit_count > 0 { depth += 1; }
    if leaf_count > 0 { depth += 1; }
    depth.saturating_sub(1).max(2)
}
