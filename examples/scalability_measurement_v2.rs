// SPDX-License-Identifier: GPL-2.0-or-later
//
// Scalability Measurement V2
//
// Fixed version that properly tracks routers during creation
//
// Uses SCION specification defaults (draft-dekater-scion-controlplane-10):
//   - max_pcbs = 50 (recommended best PCBs set size)
//   - Topology-aware beaconing rounds based on hierarchy depth
//   - Registration limit = 50 segments

use bgpsim::event::BasicEventQueue;
use bgpsim::ospf::GlobalOspf;
use bgpsim::prelude::*;
use bgpsim::scion::{IsdAs, ScionLinkType};
use bgpsim::types::SimplePrefix;
use std::time::Instant;

struct IsdTopology {
    isd_number: u16,
    cores: Vec<RouterId>,
    transits: Vec<RouterId>,
    leaves: Vec<RouterId>,
}

fn main() -> Result<(), NetworkError> {
    println!("=== SCION Scalability Measurement V2 ===");
    println!("System: 24 cores, 32GB RAM");
    println!("Spec: draft-dekater-scion-controlplane-10");
    println!("  max_pcbs = 50 (spec-recommended)");
    println!("  topology-aware beaconing rounds\n");

    // Test sizes (100K enabled with Phase 2 spec-compliant selection!)
    let sizes = vec![100, 1000, 10000, 100000];

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

    println!(
        "Topology: {} ISDs, {} core/ISD, {} transit/ISD, {} leaf/ISD",
        num_isds, core_per_isd, transit_per_isd, leaf_per_isd
    );

    // 1. Topology Creation (creates routers, links, and enables SCION all together)
    let start = Instant::now();
    let mut net =
        create_and_configure_topology(num_isds, core_per_isd, transit_per_isd, leaf_per_isd)?;
    let topo_time = start.elapsed();

    let actual_routers = net.indices().count();
    println!(
        "1. Topology creation & SCION enablement: {:.3}s",
        topo_time.as_secs_f64()
    );
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
    println!(
        "   SCION enabled: {} routers ({} core)",
        scion_count, core_count
    );

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
    println!(
        "2. Beaconing (interleaved): {:.3}s",
        beaconing_time.as_secs_f64()
    );
    println!(
        "   - Inter-ISD rounds: {} (for {} ISDs)",
        inter_isd_rounds, num_isds
    );
    println!(
        "   - Intra-ISD rounds per iteration: {} (depth+1)",
        intra_rounds_per_iter
    );
    println!("   - Total core PCBs: {}", total_core_pcbs);
    println!("   - Total intra PCBs: {}", total_intra_pcbs);

    // 3. Registration
    let start = Instant::now();
    let (up, down, core) = net.scion_registration_round(50)?;
    let registration_time = start.elapsed();
    println!("3. Registration: {:.3}s", registration_time.as_secs_f64());
    println!(
        "   Registered: {} up, {} down, {} core segments",
        up, down, core
    );

    // 4. Path Lookup
    println!("4. Path lookup (preparing...)");

    // Adaptive query count based on network size
    let query_routers = if size >= 10000 {
        3 // For 10k+: 3 routers = 3 queries
    } else if size >= 1000 {
        4 // For 1k: 4 routers = 6 queries
    } else {
        5 // For <1k: 5 routers = 10 queries
    };

    let routers: Vec<_> = net
        .indices()
        .take(query_routers.min(actual_routers))
        .collect();
    println!("   Selected {} routers for testing", routers.len());

    let mut lookup_times = Vec::new();
    let max_queries = routers.len().min(query_routers);
    let total_queries = max_queries * (max_queries - 1) / 2;

    println!(
        "   Will test {} queries (pairs from first {} routers)",
        total_queries, max_queries
    );

    for i in 0..routers.len().min(query_routers) {
        for j in (i + 1)..routers.len().min(query_routers) {
            let query_num = lookup_times.len() + 1;
            println!(
                "   Query {}/{}: router[{}] -> router[{}]",
                query_num, total_queries, i, j
            );

            let start = Instant::now();
            println!("      Looking up path segments (spec-compliant)...");

            // Use spec-compliant segment lookup
            let segments = net.scion_lookup_path_segments(routers[i], routers[j])?;
            let lookup_time = start.elapsed();

            println!(
                "      -> Found {} up, {} core, {} down segments in {:.6}s",
                segments.up_segments.len(),
                segments.core_segments.len(),
                segments.down_segments.len(),
                lookup_time.as_secs_f64()
            );

            // Estimate potential paths (without materializing them all)
            let potential_paths = segments.up_segments.len().max(1)
                * segments.core_segments.len().max(1)
                * segments.down_segments.len().max(1);
            println!("      -> Potential path combinations: {}", potential_paths);

            lookup_times.push(lookup_time);
        }
    }

    let avg_lookup = if !lookup_times.is_empty() {
        lookup_times.iter().sum::<std::time::Duration>() / lookup_times.len() as u32
    } else {
        std::time::Duration::from_secs(0)
    };
    println!(
        "   Average lookup time ({} queries): {:.6}s",
        lookup_times.len(),
        avg_lookup.as_secs_f64()
    );

    // Total time (batch mode)
    let total_batch = topo_time + beaconing_time + registration_time;
    println!(
        "\nTotal setup time (batch mode): {:.3}s",
        total_batch.as_secs_f64()
    );

    // === EVENT-DRIVEN BEACONING COMPARISON ===
    println!("\n{}", "─".repeat(60));
    println!("EVENT-DRIVEN BEACONING (NEW)");
    println!("{}", "─".repeat(60));

    // Create new network with same topology for fair comparison
    let start_topo = Instant::now();
    let mut net_event =
        create_and_configure_topology(num_isds, core_per_isd, transit_per_isd, leaf_per_isd)?;
    let topo_time_event = start_topo.elapsed();
    println!(
        "1. Topology creation: {:.3}s",
        topo_time_event.as_secs_f64()
    );

    // Event-driven beaconing
    let start_event_beaconing = Instant::now();
    let core_count = net_event.scion_start_beaconing(1000)?;
    let events_processed = net_event.scion_converge()?;
    let event_beaconing_time = start_event_beaconing.elapsed();

    println!(
        "2. Event-driven beaconing: {:.3}s",
        event_beaconing_time.as_secs_f64()
    );
    println!("   - Core ASes: {}", core_count);
    println!("   - Events processed: {}", events_processed);
    println!(
        "   - Speedup vs batch: {:.2}x",
        beaconing_time.as_secs_f64() / event_beaconing_time.as_secs_f64()
    );

    // Count PCBs in event-driven mode
    let mut event_total_pcbs = 0;
    for r in net_event.indices() {
        if let Ok(bs) = net_event.get_scion_beacon_store(r) {
            event_total_pcbs += bs.total_count();
        }
    }
    let batch_total_pcbs = total_core_pcbs + total_intra_pcbs;
    println!(
        "   - Total PCBs collected: {} (batch: {})",
        event_total_pcbs, batch_total_pcbs
    );

    // Registration
    let start_reg = Instant::now();
    let (up_e, down_e, core_e) = net_event.scion_registration_round(50)?;
    let reg_time_event = start_reg.elapsed();
    println!("3. Registration: {:.3}s", reg_time_event.as_secs_f64());
    println!(
        "   Registered: {} up, {} down, {} core segments",
        up_e, down_e, core_e
    );

    let total_event = topo_time_event + event_beaconing_time + reg_time_event;
    println!(
        "\nTotal setup time (event-driven): {:.3}s",
        total_event.as_secs_f64()
    );
    println!(
        "Overall speedup: {:.2}x",
        total_batch.as_secs_f64() / total_event.as_secs_f64()
    );

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
        let isd_number = (isd_idx + 1) as u16; // ISDs are 1-based
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
            net.enable_scion(router, IsdAs::new(isd_number, asn), true)?; // Enable SCION immediately
            isd_topo.cores.push(router);
        }

        // Transit ASes
        for i in 0..transit_per_isd {
            let asn = (isd_number as u64 * 100) + (core_per_isd + i) as u64 + 10;
            let router =
                net.add_router(&format!("ISD{}-Transit{}", isd_number, i), ASN(asn as u32));
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
            for j in (i + 1)..isd_topo.cores.len() {
                net.add_link(isd_topo.cores[i], isd_topo.cores[j])?;
                net.configure_scion_link(
                    isd_topo.cores[i],
                    isd_topo.cores[j],
                    ScionLinkType::Core,
                )?;
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
            net.configure_scion_link(
                all_isds[i].cores[0],
                all_isds[next].cores[0],
                ScionLinkType::Core,
            )?;
        }
    }

    Ok(net)
}

fn estimate_hierarchy_depth(core_count: usize, transit_count: usize, leaf_count: usize) -> usize {
    let mut depth: usize = 0;
    if core_count > 0 {
        depth += 1;
    }
    if transit_count > 0 {
        depth += 1;
    }
    if leaf_count > 0 {
        depth += 1;
    }
    depth.saturating_sub(1).max(2)
}
