// Test 10K routers with optimizations
use bgpsim::prelude::*;
use bgpsim::event::BasicEventQueue;
use bgpsim::types::SimplePrefix;
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::{IsdAs, ScionLinkType};
use std::time::Instant;

fn main() -> Result<(), NetworkError> {
    println!("Testing 10K routers with Phase 1 optimizations");
    println!("  - HashSet for AS path lookups (O(1) instead of O(n))");
    println!("  - Hop count limit (MAX_HOPS = 8)\n");

    let size = 10_000;
    let num_isds = 100;
    let core_per_isd = 5;
    let transit_per_isd = 25;
    let leaf_per_isd = 70;

    // Create topology
    let start_topo = Instant::now();
    let mut net = create_topology(num_isds, core_per_isd, transit_per_isd, leaf_per_isd)?;
    let topo_time = start_topo.elapsed();
    println!("1. Topology creation: {:.3}s", topo_time.as_secs_f64());

    // Event-driven beaconing
    let start_beaconing = Instant::now();
    let core_count = net.scion_start_beaconing(1000)?;
    let events_processed = net.scion_converge()?;
    let beaconing_time = start_beaconing.elapsed();

    println!("2. Event-driven beaconing: {:.3}s", beaconing_time.as_secs_f64());
    println!("   - Core ASes: {}", core_count);
    println!("   - Events processed: {}", events_processed);

    // Count PCBs
    let mut total_pcbs = 0;
    for r in net.indices() {
        if let Ok(bs) = net.get_scion_beacon_store(r) {
            total_pcbs += bs.total_count();
        }
    }
    println!("   - Total PCBs: {}", total_pcbs);

    // Registration
    let start_reg = Instant::now();
    let (up, down, core) = net.scion_registration_round(50)?;
    let reg_time = start_reg.elapsed();
    println!("3. Registration: {:.3}s", reg_time.as_secs_f64());
    println!("   Registered: {} up, {} down, {} core", up, down, core);

    let total_time = topo_time + beaconing_time + reg_time;
    println!("\nTotal time: {:.3}s", total_time.as_secs_f64());

    // Compare to baseline
    println!("\n--- Comparison to Baseline ---");
    println!("Baseline (no optimizations): 14.806s");
    println!("  - Events: 5,369,198");
    println!("  - PCBs: 4,390,686");
    println!("\nOptimized (Phase 1):");
    println!("  - Time: {:.3}s", total_time.as_secs_f64());
    println!("  - Events: {}", events_processed);
    println!("  - PCBs: {}", total_pcbs);
    println!("  - Speedup: {:.2}x", 14.806 / total_time.as_secs_f64());
    println!("  - Event reduction: {:.1}%", (1.0 - events_processed as f64 / 5_369_198.0) * 100.0);

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

    struct IsdTopo {
        cores: Vec<RouterId>,
        transits: Vec<RouterId>,
        leaves: Vec<RouterId>,
    }

    // Create routers
    for isd_idx in 0..num_isds {
        let isd_number = (isd_idx + 1) as u16;
        let mut topo = IsdTopo {
            cores: Vec::new(),
            transits: Vec::new(),
            leaves: Vec::new(),
        };

        for i in 0..core_per_isd {
            let asn = (isd_number as u64 * 100) + i as u64 + 10;
            let r = net.add_router(&format!("ISD{}-C{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(r, IsdAs::new(isd_number, asn), true)?;
            topo.cores.push(r);
        }

        for i in 0..transit_per_isd {
            let asn = (isd_number as u64 * 100) + (core_per_isd + i) as u64 + 10;
            let r = net.add_router(&format!("ISD{}-T{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(r, IsdAs::new(isd_number, asn), false)?;
            topo.transits.push(r);
        }

        for i in 0..leaf_per_isd {
            let asn = (isd_number as u64 * 100) + (core_per_isd + transit_per_isd + i) as u64 + 10;
            let r = net.add_router(&format!("ISD{}-L{}", isd_number, i), ASN(asn as u32));
            net.enable_scion(r, IsdAs::new(isd_number, asn), false)?;
            topo.leaves.push(r);
        }

        all_isds.push(topo);
    }

    // Create links
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

    // Inter-ISD core links (ring)
    for i in 0..all_isds.len() {
        let next = (i + 1) % all_isds.len();
        if next != i && !all_isds[i].cores.is_empty() && !all_isds[next].cores.is_empty() {
            net.add_link(all_isds[i].cores[0], all_isds[next].cores[0])?;
            net.configure_scion_link(all_isds[i].cores[0], all_isds[next].cores[0], ScionLinkType::Core)?;
        }
    }

    Ok(net)
}
