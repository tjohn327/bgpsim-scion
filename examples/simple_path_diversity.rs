// SPDX-License-Identifier: GPL-2.0-or-later
//
// Simple Path Diversity Test
//
// Test path diversity with single ISD to understand max_pcbs impact

use bgpsim::prelude::*;
use bgpsim::event::BasicEventQueue;
use bgpsim::types::SimplePrefix;
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::{IsdAs, IsdNumber, ScionLinkType};
use std::time::Instant;

fn main() -> Result<(), NetworkError> {
    println!("=== Simple SCION Path Diversity Test ===\n");

    // Create a moderately complex single-ISD topology
    // with multiple paths between ASes
    let mut net: Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf> = Network::default();

    // ISD 1 with 1 core, 4 transit, 8 leaf ASes = 13 total
    let core1 = net.add_router("Core1", ASN(101));

    let t1 = net.add_router("T1", ASN(110));
    let t2 = net.add_router("T2", ASN(111));
    let t3 = net.add_router("T3", ASN(112));
    let t4 = net.add_router("T4", ASN(113));

    let l1 = net.add_router("L1", ASN(120));
    let l2 = net.add_router("L2", ASN(121));
    let l3 = net.add_router("L3", ASN(122));
    let l4 = net.add_router("L4", ASN(123));
    let l5 = net.add_router("L5", ASN(124));
    let l6 = net.add_router("L6", ASN(125));
    let l7 = net.add_router("L7", ASN(126));
    let l8 = net.add_router("L8", ASN(127));

    // Connect topology with redundant paths
    // Core to all transits
    for &t in &[t1, t2, t3, t4] {
        net.add_link(core1, t)?;
    }

    // Each leaf connects to 2 transits (creates path diversity)
    net.add_link(t1, l1)?; net.add_link(t2, l1)?;
    net.add_link(t1, l2)?; net.add_link(t2, l2)?;
    net.add_link(t2, l3)?; net.add_link(t3, l3)?;
    net.add_link(t2, l4)?; net.add_link(t3, l4)?;
    net.add_link(t3, l5)?; net.add_link(t4, l5)?;
    net.add_link(t3, l6)?; net.add_link(t4, l6)?;
    net.add_link(t4, l7)?; net.add_link(t1, l7)?;
    net.add_link(t4, l8)?; net.add_link(t1, l8)?;

    println!("Created topology: 1 core, 4 transit, 8 leaf = 13 ASes");
    println!("Each leaf has 2 upstream paths for diversity\n");

    // Enable SCION
    net.enable_scion(core1, IsdAs::new(IsdNumber(1), 101u64), true)?;
    for &t in &[t1, t2, t3, t4] {
        let asn = net.get_router(t)?.asn();
        net.enable_scion(t, IsdAs::new(IsdNumber(1), asn.0 as u64), false)?;
    }
    for &l in &[l1, l2, l3, l4, l5, l6, l7, l8] {
        let asn = net.get_router(l)?.asn();
        net.enable_scion(l, IsdAs::new(IsdNumber(1), asn.0 as u64), false)?;
    }

    // Configure links
    let all_routers: Vec<_> = net.indices().collect();
    for &router in &all_routers {
        let r_is_core = net.get_router(router)?.scion().map(|s| s.is_core).unwrap_or(false);

        let neighbors: Vec<_> = net.ospf_network().neighbors(router)
            .map(|e| e.src())
            .collect();

        for neighbor in neighbors {
            if router.index() >= neighbor.index() {
                continue;
            }

            let n_is_core = net.get_router(neighbor)?.scion().map(|s| s.is_core).unwrap_or(false);

            let link_type = if r_is_core || n_is_core {
                ScionLinkType::ParentChild
            } else {
                ScionLinkType::Peering
            };

            net.configure_scion_link(router, neighbor, link_type)?;
        }
    }

    println!("SCION enabled and links configured\n");

    // Test different max_pcbs values
    let max_pcbs_values = vec![2, 5, 10, 20, 50, 100];

    println!("{:<10} {:<15} {:<15} {:<15} {:<15}",
             "max_pcbs", "Beacon Time", "Reg Time", "L1→L5 Paths", "All Segments");
    println!("{}", "-".repeat(70));

    for &max_pcbs in &max_pcbs_values {
        // Reset beacons (create new network for clean state)
        let mut test_net = net.clone();

        // Run beaconing
        let beacon_start = Instant::now();
        test_net.scion_core_beaconing(1000)?;

        // Multiple rounds to propagate through 3-tier hierarchy
        for _ in 0..5 {
            test_net.scion_intra_isd_beaconing(1000, max_pcbs)?;
        }
        let beacon_time = beacon_start.elapsed();

        // Registration
        let reg_start = Instant::now();
        let (up, down, core) = test_net.scion_registration_round(max_pcbs)?;
        let reg_time = reg_start.elapsed();

        // Check specific path (L1 to L5 - should have multiple paths)
        let paths_l1_l5 = test_net.scion_lookup_paths(l1, l5)?;

        // Count average paths across all leaf pairs
        let leaves = vec![l1, l2, l3, l4, l5, l6, l7, l8];
        let mut total_paths = 0;
        let mut count = 0;
        for i in 0..leaves.len() {
            for j in (i+1)..leaves.len() {
                if let Ok(paths) = test_net.scion_lookup_paths(leaves[i], leaves[j]) {
                    total_paths += paths.len();
                    count += 1;
                }
            }
        }
        let avg_paths = if count > 0 { total_paths as f64 / count as f64 } else { 0.0 };

        println!("{:<10} {:<15.4} {:<15.4} {:<15} {:<15}",
                 max_pcbs,
                 beacon_time.as_secs_f64(),
                 reg_time.as_secs_f64(),
                 paths_l1_l5.len(),
                 up + down + core);

        if max_pcbs == 2 || max_pcbs == 100 {
            println!("  └─ Detail: {} up, {} down, {} core segments | Avg paths: {:.1}",
                     up, down, core, avg_paths);
        }
    }

    println!("\n=== Analysis ===");
    println!("Look for the 'knee' in the curve where increasing max_pcbs");
    println!("doesn't significantly increase path diversity.");

    Ok(())
}
