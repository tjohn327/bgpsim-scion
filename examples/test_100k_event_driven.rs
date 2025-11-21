// SPDX-License-Identifier: GPL-2.0-or-later
//
// 100K Router Event-Driven Test (NO BATCH MODE)
//
// Tests ONLY the event-driven beaconing with Phase 2 optimizations
// Skips the slow batch mode baseline to save time

use bgpsim::prelude::*;
use bgpsim::event::BasicEventQueue;
use bgpsim::types::SimplePrefix;
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::{IsdAs, ScionLinkType};
use std::time::Instant;

struct IsdTopology {
    cores: Vec<RouterId>,
    transits: Vec<RouterId>,
    leaves: Vec<RouterId>,
}

fn main() -> Result<(), NetworkError> {
    println!("=== 100K Router Event-Driven Test (Phase 2) ===");
    println!("Testing ONLY event-driven beaconing with spec-compliant selection");
    println!("Skipping batch mode (too slow for 100K)\n");

    let size = 100_000;

    // Calculate topology parameters
    let num_isds = ((size as f64).sqrt().ceil() as usize).max(1);
    let ases_per_isd = size / num_isds;
    let core_per_isd = (ases_per_isd as f64 * 0.05).ceil().max(2.0) as usize;
    let transit_per_isd = (ases_per_isd as f64 * 0.25).ceil() as usize;
    let leaf_per_isd = ases_per_isd.saturating_sub(core_per_isd + transit_per_isd);

    println!("Topology: {} ISDs, {} core/ISD, {} transit/ISD, {} leaf/ISD\n",
             num_isds, core_per_isd, transit_per_isd, leaf_per_isd);

    // 1. Topology Creation
    println!("Phase 1: Creating topology...");
    let start_topo = Instant::now();
    let mut net = create_topology(num_isds, core_per_isd, transit_per_isd, leaf_per_isd)?;
    let topo_time = start_topo.elapsed();

    let actual_routers = net.indices().count();
    let mut core_count = 0;
    for r in net.indices() {
        if let Ok(router) = net.get_router(r) {
            if let Some(scion) = router.scion() {
                if scion.is_core {
                    core_count += 1;
                }
            }
        }
    }

    println!("✓ Topology created in {:.3}s", topo_time.as_secs_f64());
    println!("  - {} total routers", actual_routers);
    println!("  - {} core ASes\n", core_count);

    // 2. Event-Driven Beaconing (THE CRITICAL TEST!)
    println!("Phase 2: Event-driven beaconing...");
    let start_beaconing = Instant::now();
    let core_ases = net.scion_start_beaconing(1000)?;
    let events_processed = net.scion_converge()?;
    let beaconing_time = start_beaconing.elapsed();

    println!("✓ Beaconing complete in {:.3}s", beaconing_time.as_secs_f64());
    println!("  - Core ASes: {}", core_ases);
    println!("  - Events processed: {}", events_processed);
    println!("  - Events/second: {:.0}", events_processed as f64 / beaconing_time.as_secs_f64());

    // Count PCBs
    let mut total_pcbs = 0;
    for r in net.indices() {
        if let Ok(bs) = net.get_scion_beacon_store(r) {
            total_pcbs += bs.total_count();
        }
    }
    println!("  - Total PCBs collected: {}\n", total_pcbs);

    // 3. Registration
    println!("Phase 3: Registering path segments...");
    let start_reg = Instant::now();
    let (up, down, core_segs) = net.scion_registration_round(50)?;
    let reg_time = start_reg.elapsed();

    println!("✓ Registration complete in {:.3}s", reg_time.as_secs_f64());
    println!("  - Up segments: {}", up);
    println!("  - Down segments: {}", down);
    println!("  - Core segments: {}\n", core_segs);

    // 4. Path Lookup Test
    println!("Phase 4: Testing path lookup...");
    let routers: Vec<_> = net.indices().take(2).collect();

    if routers.len() >= 2 {
        let start_lookup = Instant::now();
        let segments = net.scion_lookup_path_segments(routers[0], routers[1])?;
        let lookup_time = start_lookup.elapsed();

        println!("✓ Path lookup test complete in {:.6}s", lookup_time.as_secs_f64());
        println!("  - Up segments found: {}", segments.up_segments.len());
        println!("  - Core segments found: {}", segments.core_segments.len());
        println!("  - Down segments found: {}\n", segments.down_segments.len());
    }

    // Summary
    let total_time = topo_time + beaconing_time + reg_time;
    println!("=== RESULTS ===");
    println!("Total time: {:.3}s", total_time.as_secs_f64());
    println!("  - Topology creation: {:.3}s ({:.1}%)",
             topo_time.as_secs_f64(),
             100.0 * topo_time.as_secs_f64() / total_time.as_secs_f64());
    println!("  - Event-driven beaconing: {:.3}s ({:.1}%)",
             beaconing_time.as_secs_f64(),
             100.0 * beaconing_time.as_secs_f64() / total_time.as_secs_f64());
    println!("  - Registration: {:.3}s ({:.1}%)",
             reg_time.as_secs_f64(),
             100.0 * reg_time.as_secs_f64() / total_time.as_secs_f64());

    println!("\n✅ SUCCESS: 100K router simulation completed!");
    println!("   Phase 2 optimizations enabled scalability to 100K routers");

    Ok(())
}

fn create_topology(
    num_isds: usize,
    core_per_isd: usize,
    transit_per_isd: usize,
    leaf_per_isd: usize,
) -> Result<Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>, NetworkError> {
    let mut net = Network::default();
    let mut all_isds = Vec::new();

    // Create routers for each ISD
    for isd_idx in 0..num_isds {
        let isd_number = (isd_idx + 1) as u16;
        let mut topo = IsdTopology {
            cores: Vec::new(),
            transits: Vec::new(),
            leaves: Vec::new(),
        };

        // Core ASes
        for i in 0..core_per_isd {
            let asn = (isd_number as u64 * 100) + i as u64 + 10;
            let r = net.add_router(&format!("ISD{}-C{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(r, IsdAs::new(isd_number, asn), true)?;
            topo.cores.push(r);
        }

        // Transit ASes
        for i in 0..transit_per_isd {
            let asn = (isd_number as u64 * 100) + (core_per_isd + i) as u64 + 10;
            let r = net.add_router(&format!("ISD{}-T{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(r, IsdAs::new(isd_number, asn), false)?;
            topo.transits.push(r);
        }

        // Leaf ASes
        for i in 0..leaf_per_isd {
            let asn = (isd_number as u64 * 100) + (core_per_isd + transit_per_isd + i) as u64 + 10;
            let r = net.add_router(&format!("ISD{}-L{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(r, IsdAs::new(isd_number, asn), false)?;
            topo.leaves.push(r);
        }

        all_isds.push(topo);
    }

    // Create links within each ISD
    for topo in &all_isds {
        // Core mesh
        for i in 0..topo.cores.len() {
            for j in (i+1)..topo.cores.len() {
                net.add_link(topo.cores[i], topo.cores[j])?;
                net.configure_scion_link(topo.cores[i], topo.cores[j], ScionLinkType::Core)?;
            }
        }

        // Core to transit
        for (i, &transit) in topo.transits.iter().enumerate() {
            let core = topo.cores[i % topo.cores.len()];
            net.add_link(core, transit)?;
            net.configure_scion_link(core, transit, ScionLinkType::ParentChild)?;
        }

        // Transit to leaf
        for (i, &leaf) in topo.leaves.iter().enumerate() {
            let transit = topo.transits[i % topo.transits.len()];
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
