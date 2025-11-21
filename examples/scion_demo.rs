// SPDX-License-Identifier: GPL-2.0-or-later
//
// SCION Demonstration: Complex Multi-ISD Topology
//
// Comprehensive example showcasing bgpsim-scion capabilities:
// - 3 ISDs with realistic hierarchy (Core → Transit → Leaf)
// - Multiple path types (intra-ISD, inter-ISD)
// - Detailed topology visualization
// - Verbose beaconing and registration output
// - Path segment analysis and statistics

use bgpsim::event::BasicEventQueue;
use bgpsim::ospf::GlobalOspf;
use bgpsim::prelude::*;
use bgpsim::scion::{IsdAs, ScionLinkType};
use bgpsim::types::SimplePrefix;
use std::collections::HashMap;

type Net = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>;

fn main() -> Result<(), NetworkError> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║     SCION Control Plane Simulation - Demonstration          ║");
    println!("║              Complex Multi-ISD Topology                      ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Build the topology
    let (mut net, topology) = build_complex_topology()?;

    // Display topology
    print_topology(&topology);

    // Run beaconing with verbose output
    run_beaconing_with_stats(&mut net)?;

    // Register segments
    register_segments_with_stats(&mut net)?;

    // Demonstrate path lookups
    demonstrate_path_lookups(&net, &topology)?;

    // Final statistics
    print_final_statistics(&net, &topology)?;

    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║                    Demonstration Complete                    ║");
    println!("╚══════════════════════════════════════════════════════════════╝");

    Ok(())
}

#[derive(Debug)]
struct Topology {
    isds: HashMap<u16, IsdTopology>,
}

#[derive(Debug)]
struct IsdTopology {
    isd: u16,
    cores: Vec<(RouterId, String)>,
    transits: Vec<(RouterId, String)>,
    leaves: Vec<(RouterId, String)>,
}

fn build_complex_topology() -> Result<(Net, Topology), NetworkError> {
    println!("═══ Building Complex Topology ═══\n");

    let mut net = Net::default();
    let mut topology = Topology {
        isds: HashMap::new(),
    };

    // ISD 1: North America (3 core, 4 transit, 6 leaf)
    println!("Building ISD 1 (North America):");
    let isd1 = build_isd(&mut net, 1, "NA", 3, 4, 6)?;
    topology.isds.insert(1, isd1);

    // ISD 2: Europe (3 core, 3 transit, 4 leaf)
    println!("\nBuilding ISD 2 (Europe):");
    let isd2 = build_isd(&mut net, 2, "EU", 3, 3, 4)?;
    topology.isds.insert(2, isd2);

    // ISD 3: Asia (2 core, 3 transit, 5 leaf)
    println!("\nBuilding ISD 3 (Asia):");
    let isd3 = build_isd(&mut net, 3, "AS", 2, 3, 5)?;
    topology.isds.insert(3, isd3);

    // Create inter-ISD links
    println!("\n═══ Creating Inter-ISD Links ═══\n");
    create_inter_isd_links(&mut net, &topology)?;

    let total_routers = net.indices().count();
    println!("\n✓ Topology created: {} total routers\n", total_routers);

    Ok((net, topology))
}

fn build_isd(
    net: &mut Net,
    isd: u16,
    region: &str,
    n_core: usize,
    n_transit: usize,
    n_leaf: usize,
) -> Result<IsdTopology, NetworkError> {
    let mut isd_topo = IsdTopology {
        isd,
        cores: Vec::new(),
        transits: Vec::new(),
        leaves: Vec::new(),
    };

    // Create core ASes
    println!("  Core ASes:");
    for i in 0..n_core {
        let asn = (isd as u64 * 1000) + 100 + i as u64;
        let name = format!("{}-Core{}", region, i + 1);
        let router = net.add_router(&name, asn as u32);
        net.enable_scion(router, IsdAs::new(isd, asn), true)?;
        println!("    ✓ {}-{} (core)", isd, asn);
        isd_topo.cores.push((router, name));
    }

    // Create transit ASes
    println!("  Transit ASes:");
    for i in 0..n_transit {
        let asn = (isd as u64 * 1000) + 200 + i as u64;
        let name = format!("{}-Transit{}", region, i + 1);
        let router = net.add_router(&name, asn as u32);
        net.enable_scion(router, IsdAs::new(isd, asn), false)?;
        println!("    ✓ {}-{}", isd, asn);
        isd_topo.transits.push((router, name));
    }

    // Create leaf ASes
    println!("  Leaf ASes:");
    for i in 0..n_leaf {
        let asn = (isd as u64 * 1000) + 300 + i as u64;
        let name = format!("{}-Leaf{}", region, i + 1);
        let router = net.add_router(&name, asn as u32);
        net.enable_scion(router, IsdAs::new(isd, asn), false)?;
        println!("    ✓ {}-{}", isd, asn);
        isd_topo.leaves.push((router, name));
    }

    // Create intra-ISD links
    create_intra_isd_links(net, &isd_topo)?;

    Ok(isd_topo)
}

fn create_intra_isd_links(net: &mut Net, isd: &IsdTopology) -> Result<(), NetworkError> {
    // Core mesh (full connectivity)
    for i in 0..isd.cores.len() {
        for j in (i + 1)..isd.cores.len() {
            net.add_link(isd.cores[i].0, isd.cores[j].0)?;
            net.configure_scion_link(isd.cores[i].0, isd.cores[j].0, ScionLinkType::Core)?;
        }
    }

    // Core to Transit (each transit connects to 1-2 cores)
    for (i, transit) in isd.transits.iter().enumerate() {
        let core1 = isd.cores[i % isd.cores.len()].0;
        net.add_link(core1, transit.0)?;
        net.configure_scion_link(core1, transit.0, ScionLinkType::ParentChild)?;

        // Some transits connect to multiple cores for redundancy
        if i % 2 == 0 && isd.cores.len() > 1 {
            let core2 = isd.cores[(i + 1) % isd.cores.len()].0;
            net.add_link(core2, transit.0)?;
            net.configure_scion_link(core2, transit.0, ScionLinkType::ParentChild)?;
        }
    }

    // Transit to Leaf (each leaf connects to 1-2 transits)
    for (i, leaf) in isd.leaves.iter().enumerate() {
        let transit1 = isd.transits[i % isd.transits.len()].0;
        net.add_link(transit1, leaf.0)?;
        net.configure_scion_link(transit1, leaf.0, ScionLinkType::ParentChild)?;

        // Some leaves have redundant connections
        if i % 3 == 0 && isd.transits.len() > 1 {
            let transit2 = isd.transits[(i + 1) % isd.transits.len()].0;
            net.add_link(transit2, leaf.0)?;
            net.configure_scion_link(transit2, leaf.0, ScionLinkType::ParentChild)?;
        }
    }

    // Peering between some transits
    if isd.transits.len() >= 2 {
        for i in 0..isd.transits.len().min(2) {
            for j in (i + 1)..isd.transits.len().min(3) {
                net.add_link(isd.transits[i].0, isd.transits[j].0)?;
                net.configure_scion_link(
                    isd.transits[i].0,
                    isd.transits[j].0,
                    ScionLinkType::Peering,
                )?;
            }
        }
    }

    Ok(())
}

fn create_inter_isd_links(net: &mut Net, topology: &Topology) -> Result<(), NetworkError> {
    // ISD 1 <-> ISD 2: 2 core links
    let isd1 = &topology.isds[&1];
    let isd2 = &topology.isds[&2];

    net.add_link(isd1.cores[0].0, isd2.cores[0].0)?;
    net.configure_scion_link(isd1.cores[0].0, isd2.cores[0].0, ScionLinkType::Core)?;
    println!(
        "  ✓ ISD 1-{} ↔ ISD 2-{}",
        extract_asn(&isd1.cores[0].1),
        extract_asn(&isd2.cores[0].1)
    );

    net.add_link(isd1.cores[1].0, isd2.cores[1].0)?;
    net.configure_scion_link(isd1.cores[1].0, isd2.cores[1].0, ScionLinkType::Core)?;
    println!(
        "  ✓ ISD 1-{} ↔ ISD 2-{}",
        extract_asn(&isd1.cores[1].1),
        extract_asn(&isd2.cores[1].1)
    );

    // ISD 2 <-> ISD 3: 2 core links
    let isd3 = &topology.isds[&3];

    net.add_link(isd2.cores[0].0, isd3.cores[0].0)?;
    net.configure_scion_link(isd2.cores[0].0, isd3.cores[0].0, ScionLinkType::Core)?;
    println!(
        "  ✓ ISD 2-{} ↔ ISD 3-{}",
        extract_asn(&isd2.cores[0].1),
        extract_asn(&isd3.cores[0].1)
    );

    net.add_link(isd2.cores[2].0, isd3.cores[1].0)?;
    net.configure_scion_link(isd2.cores[2].0, isd3.cores[1].0, ScionLinkType::Core)?;
    println!(
        "  ✓ ISD 2-{} ↔ ISD 3-{}",
        extract_asn(&isd2.cores[2].1),
        extract_asn(&isd3.cores[1].1)
    );

    // ISD 1 <-> ISD 3: 1 core link
    net.add_link(isd1.cores[2].0, isd3.cores[0].0)?;
    net.configure_scion_link(isd1.cores[2].0, isd3.cores[0].0, ScionLinkType::Core)?;
    println!(
        "  ✓ ISD 1-{} ↔ ISD 3-{}",
        extract_asn(&isd1.cores[2].1),
        extract_asn(&isd3.cores[0].1)
    );

    Ok(())
}

fn extract_asn(name: &str) -> &str {
    name.split('-').nth(1).unwrap_or("?")
}

fn print_topology(topology: &Topology) {
    println!("\n═══ Topology Summary ═══\n");

    let mut total_cores = 0;
    let mut total_transits = 0;
    let mut total_leaves = 0;

    for isd_num in [1, 2, 3] {
        if let Some(isd) = topology.isds.get(&isd_num) {
            println!("ISD {} ({}):", isd_num, get_region_name(isd_num));
            println!("  └─ {} Core ASes", isd.cores.len());
            println!("  └─ {} Transit ASes", isd.transits.len());
            println!("  └─ {} Leaf ASes", isd.leaves.len());
            println!(
                "  └─ Total: {} ASes\n",
                isd.cores.len() + isd.transits.len() + isd.leaves.len()
            );

            total_cores += isd.cores.len();
            total_transits += isd.transits.len();
            total_leaves += isd.leaves.len();
        }
    }

    println!("Network-wide:");
    println!("  └─ {} Core ASes", total_cores);
    println!("  └─ {} Transit ASes", total_transits);
    println!("  └─ {} Leaf ASes", total_leaves);
    println!(
        "  └─ {} Total ASes",
        total_cores + total_transits + total_leaves
    );
    println!("  └─ 5 Inter-ISD core links\n");
}

fn get_region_name(isd: u16) -> &'static str {
    match isd {
        1 => "North America",
        2 => "Europe",
        3 => "Asia",
        _ => "Unknown",
    }
}

fn run_beaconing_with_stats(net: &mut Net) -> Result<(), NetworkError> {
    println!("═══ Running Beaconing ═══\n");

    // Choose mode: Use environment variable or default to event-driven
    let use_event_driven = std::env::var("SCION_BATCH_MODE").is_err();

    if use_event_driven {
        println!("Mode: EVENT-DRIVEN (new)");
        println!("  (set SCION_BATCH_MODE=1 to use old batch mode)\n");

        let start = std::time::Instant::now();
        let core_count = net.scion_start_beaconing(1000)?;
        let events_processed = net.scion_converge()?;
        let elapsed = start.elapsed();

        println!("Beaconing Summary (Event-Driven):");
        println!("  ├─ Core ASes: {}", core_count);
        println!("  ├─ Events processed: {}", events_processed);
        println!("  ├─ Time: {:.3}s", elapsed.as_secs_f64());

        // Count total PCBs
        let mut total_pcbs = 0;
        for router_id in net.indices() {
            if let Ok(bs) = net.get_scion_beacon_store(router_id) {
                total_pcbs += bs.total_count();
            }
        }
        println!("  └─ Total PCBs stored: {}\n", total_pcbs);
    } else {
        println!("Mode: BATCH (legacy)\n");

        let mut timestamp = 1000u32;
        let mut total_core_pcbs = 0;
        let mut total_intra_pcbs = 0;

        // Need 3 rounds for 3-ISD topology
        let rounds = 3;
        let intra_rounds_per_iter = 3; // Depth: core → transit → leaf

        let start = std::time::Instant::now();
        for round in 1..=rounds {
            println!("Round {}:", round);

            // Core beaconing
            let core_pcbs = net.scion_core_beaconing(timestamp)?;
            total_core_pcbs += core_pcbs;
            println!("  ├─ Core beaconing: {} PCBs created", core_pcbs);
            timestamp += 1;

            // Intra-ISD beaconing (multiple rounds to reach leaves)
            for sub_round in 1..=intra_rounds_per_iter {
                let intra_pcbs = net.scion_intra_isd_beaconing(timestamp, 50)?;
                total_intra_pcbs += intra_pcbs;
                println!(
                    "  ├─ Intra-ISD sub-round {}: {} PCBs propagated",
                    sub_round, intra_pcbs
                );
                timestamp += 1;
            }
            println!("  └─ Round {} complete\n", round);
        }
        let elapsed = start.elapsed();

        println!("Beaconing Summary (Batch Mode):");
        println!("  ├─ Total core PCBs: {}", total_core_pcbs);
        println!("  ├─ Total intra-ISD PCBs: {}", total_intra_pcbs);
        println!("  ├─ Total PCBs: {}", total_core_pcbs + total_intra_pcbs);
        println!("  └─ Time: {:.3}s\n", elapsed.as_secs_f64());
    }

    Ok(())
}

fn register_segments_with_stats(net: &mut Net) -> Result<(), NetworkError> {
    println!("═══ Segment Registration ═══\n");

    let (up, down, core) = net.scion_registration_round(50)?;

    println!("Registered segments:");
    println!("  ├─ Up segments:   {} (non-core → core)", up);
    println!("  ├─ Down segments: {} (core → non-core)", down);
    println!("  ├─ Core segments: {} (core → core)", core);
    println!("  └─ Total:         {}\n", up + down + core);

    Ok(())
}

fn demonstrate_path_lookups(net: &Net, topology: &Topology) -> Result<(), NetworkError> {
    println!("═══ Path Segment Lookup Demonstration ═══\n");

    let isd1 = &topology.isds[&1];
    let isd2 = &topology.isds[&2];
    let isd3 = &topology.isds[&3];

    // Example 1: Intra-ISD path (same ISD)
    println!("Example 1: Intra-ISD Path");
    println!("  Source: {} (ISD 1, North America)", isd1.leaves[0].1);
    println!("  Dest:   {} (ISD 1, North America)", isd1.leaves[1].1);

    let segments = net.scion_lookup_path_segments(isd1.leaves[0].0, isd1.leaves[1].0)?;
    print_segment_details(&segments, "Intra-ISD");

    // Example 2: Inter-ISD path (adjacent ISDs)
    println!("\nExample 2: Inter-ISD Path (Adjacent)");
    println!("  Source: {} (ISD 1, North America)", isd1.leaves[2].1);
    println!("  Dest:   {} (ISD 2, Europe)", isd2.leaves[0].1);

    let segments = net.scion_lookup_path_segments(isd1.leaves[2].0, isd2.leaves[0].0)?;
    print_segment_details(&segments, "Inter-ISD (1→2)");

    // Example 3: Inter-ISD path (distant ISDs)
    println!("\nExample 3: Inter-ISD Path (Distant)");
    println!("  Source: {} (ISD 1, North America)", isd1.leaves[3].1);
    println!("  Dest:   {} (ISD 3, Asia)", isd3.leaves[0].1);

    let segments = net.scion_lookup_path_segments(isd1.leaves[3].0, isd3.leaves[0].0)?;
    print_segment_details(&segments, "Inter-ISD (1→3)");

    // Example 4: Path with multiple options
    println!("\nExample 4: Multi-Path Scenario");
    println!("  Source: {} (ISD 2, Europe)", isd2.leaves[1].1);
    println!("  Dest:   {} (ISD 3, Asia)", isd3.leaves[2].1);

    let segments = net.scion_lookup_path_segments(isd2.leaves[1].0, isd3.leaves[2].0)?;
    print_segment_details(&segments, "Inter-ISD (2→3)");

    // Show path construction potential
    show_path_construction_potential(&segments);

    Ok(())
}

fn print_segment_details(
    segments: &bgpsim::scion_network::PathSegments<SimplePrefix>,
    label: &str,
) {
    println!("  ┌─ {} Path Segments:", label);
    println!("  ├─ Up segments:   {}", segments.up_segments.len());
    println!("  ├─ Core segments: {}", segments.core_segments.len());
    println!("  └─ Down segments: {}", segments.down_segments.len());

    // Show example segments
    if !segments.up_segments.is_empty() {
        let seg = &segments.up_segments[0];
        let path_str: Vec<String> = seg
            .as_path
            .iter()
            .map(|hop| format!("{}-{}", hop.isd, hop.asn))
            .collect();
        println!(
            "     Example up:   {} hops: {}",
            seg.as_path.len(),
            path_str.join(" → ")
        );
    }

    if !segments.core_segments.is_empty() {
        let seg = &segments.core_segments[0];
        let path_str: Vec<String> = seg
            .as_path
            .iter()
            .map(|hop| format!("{}-{}", hop.isd, hop.asn))
            .collect();
        println!(
            "     Example core: {} hops: {}",
            seg.as_path.len(),
            path_str.join(" → ")
        );
    }

    if !segments.down_segments.is_empty() {
        let seg = &segments.down_segments[0];
        let path_str: Vec<String> = seg
            .as_path
            .iter()
            .map(|hop| format!("{}-{}", hop.isd, hop.asn))
            .collect();
        println!(
            "     Example down: {} hops: {}",
            seg.as_path.len(),
            path_str.join(" → ")
        );
    }
}

fn show_path_construction_potential(segments: &bgpsim::scion_network::PathSegments<SimplePrefix>) {
    let up_count = segments.up_segments.len().max(1);
    let core_count = segments.core_segments.len().max(1);
    let down_count = segments.down_segments.len().max(1);

    let potential_paths = up_count * core_count * down_count;

    println!("  ┌─ Path Construction Potential:");
    println!(
        "  ├─ Segment combinations: {} × {} × {} = {} possible paths",
        up_count, core_count, down_count, potential_paths
    );
    println!("  └─ Note: Endhost constructs paths from these segments");
}

fn print_final_statistics(net: &Net, topology: &Topology) -> Result<(), NetworkError> {
    println!("\n═══ Final Statistics ═══\n");

    // Count SCION-enabled routers
    let mut scion_routers = 0;
    let mut core_routers = 0;

    for router_id in net.indices() {
        if let Ok(router) = net.get_router(router_id) {
            if let Some(scion) = router.scion() {
                scion_routers += 1;
                if scion.is_core {
                    core_routers += 1;
                }
            }
        }
    }

    println!("Network Configuration:");
    println!("  ├─ Total routers: {}", net.indices().count());
    println!("  ├─ SCION-enabled: {}", scion_routers);
    println!("  └─ Core ASes:     {}", core_routers);

    println!("\nTopology Structure:");
    println!("  ├─ {} ISDs", topology.isds.len());
    println!(
        "  ├─ {} core ASes across all ISDs",
        topology.isds.values().map(|i| i.cores.len()).sum::<usize>()
    );
    println!(
        "  ├─ {} transit ASes",
        topology
            .isds
            .values()
            .map(|i| i.transits.len())
            .sum::<usize>()
    );
    println!(
        "  └─ {} leaf ASes",
        topology
            .isds
            .values()
            .map(|i| i.leaves.len())
            .sum::<usize>()
    );

    println!("\nKey Features Demonstrated:");
    println!("  ✓ Multi-ISD hierarchical topology");
    println!("  ✓ Core mesh within each ISD");
    println!("  ✓ Parent-child relationships (core→transit→leaf)");
    println!("  ✓ Peering links between transits");
    println!("  ✓ Multiple inter-ISD core links");
    println!("  ✓ Spec-compliant segment-based API");
    println!("  ✓ Path diversity through segment combinations");

    Ok(())
}
