//! SCION with MinimalOspf Example
//!
//! This example demonstrates why MinimalOspf is the preferred OSPF implementation
//! for SCION simulations:
//!
//! - **No SPT computation**: MinimalOspf assumes full mesh within AS
//! - **O(1) link updates**: Adding links doesn't trigger shortest-path computation
//! - **Ideal for SCION**: Inter-AS routing is handled by SCION hop fields
//!
//! MinimalOspf is recommended when:
//! - Simulating large SCION topologies
//! - Intra-AS routing topology isn't relevant to your research
//! - Performance is critical
//!
//! Run with: cargo run --example scion_minimal_ospf --all-features

use bgpsim::event::BasicEventQueue;
use bgpsim::network::Network;
use bgpsim::ospf::MinimalOspf;
use bgpsim::scion::*;
use bgpsim::types::{NetworkError, SimplePrefix};
use std::time::Instant;

// MinimalOspf: No SPT computation, assumes full mesh within AS
type ScionNetwork = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, MinimalOspf>;

fn main() -> Result<(), NetworkError> {
    println!("=== SCION with MinimalOspf ===\n");

    println!("MinimalOspf Benefits:");
    println!("  • No shortest-path tree computation");
    println!("  • O(1) link weight updates");
    println!("  • Assumes full mesh within each AS");
    println!("  • Perfect for SCION where inter-AS routing uses hop fields\n");

    // Measure topology build time
    let start = Instant::now();
    let (net, topology) = build_multi_isd_topology()?;
    let build_time = start.elapsed();

    println!("Topology built in {:?}", build_time);
    println!("  • {} ASes across 2 ISDs", topology.all_ases.len());
    println!("  • {} core ASes", topology.core_ases.len());
    println!();

    // Run beaconing
    let start = Instant::now();
    let net = run_beaconing(net, &topology)?;
    let beacon_time = start.elapsed();
    println!("Beaconing completed in {:?}", beacon_time);

    // Query paths
    let start = Instant::now();
    let global_db = net.build_global_path_db(&topology.all_ases)?;
    let db_time = start.elapsed();
    println!("Path database built in {:?}", db_time);
    println!("  • {} total segments\n", global_db.get_all().len());

    // Demonstrate path queries
    println!("Sample path queries:");

    let test_cases = vec![
        (topology.isd1_leaf, topology.isd1_core, "Intra-ISD (leaf → core)"),
        (topology.isd1_leaf, topology.isd2_leaf, "Inter-ISD (ISD1 → ISD2)"),
    ];

    for (src, dst, desc) in test_cases {
        let query = PathQuery {
            src,
            dst,
            max_paths: 5,
            allow_peering: true,
        };

        let src_is_core = net.is_scion_core(&src).unwrap();
        let dst_is_core = net.is_scion_core(&dst).unwrap();
        let result = construct_paths_with_peering(&query, &global_db, src_is_core, dst_is_core);

        if result.is_success() {
            println!("  {} → {}: {} paths ({})", src, dst, result.paths.len(), desc);
        }
    }

    println!("\n═══════════════════════════════════════════");
    println!("When to use MinimalOspf:");
    println!("═══════════════════════════════════════════");
    println!("  ✓ Large SCION simulations (100+ ASes)");
    println!("  ✓ Focus on inter-AS path selection");
    println!("  ✓ Intra-AS topology doesn't matter");
    println!("  ✓ Performance-critical scenarios");
    println!();
    println!("When to use GlobalOspf instead:");
    println!("  • Need accurate intra-AS routing");
    println!("  • Studying IGP-SCION interaction");
    println!("  • Need real link weights within AS");

    println!("\n=== Example Complete ===");
    Ok(())
}

struct Topology {
    isd1_core: IsdAs,
    isd1_provider: IsdAs,
    isd1_leaf: IsdAs,
    isd2_core: IsdAs,
    isd2_provider: IsdAs,
    isd2_leaf: IsdAs,
    all_ases: Vec<IsdAs>,
    core_ases: Vec<IsdAs>,
}

fn build_multi_isd_topology() -> Result<(ScionNetwork, Topology), NetworkError> {
    let mut net: ScionNetwork = Network::default();

    // ISD 1
    let isd1_core = IsdAs::new(1, 100);
    let isd1_provider = IsdAs::new(1, 200);
    let isd1_leaf = IsdAs::new(1, 300);

    // ISD 2
    let isd2_core = IsdAs::new(2, 100);
    let isd2_provider = IsdAs::new(2, 200);
    let isd2_leaf = IsdAs::new(2, 300);

    // Create routers (1 per AS for simplicity)
    let r1_core = net.add_router("1-100", 100);
    let r1_prov = net.add_router("1-200", 200);
    let r1_leaf = net.add_router("1-300", 300);

    let r2_core = net.add_router("2-100", 100);
    let r2_prov = net.add_router("2-200", 200);
    let r2_leaf = net.add_router("2-300", 300);

    // Enable SCION
    net.enable_scion_router(r1_core, isd1_core, true)?;
    net.enable_scion_router(r1_prov, isd1_provider, false)?;
    net.enable_scion_router(r1_leaf, isd1_leaf, false)?;

    net.enable_scion_router(r2_core, isd2_core, true)?;
    net.enable_scion_router(r2_prov, isd2_provider, false)?;
    net.enable_scion_router(r2_leaf, isd2_leaf, false)?;

    // Physical links (bulk add for efficiency)
    let links = vec![
        // ISD 1 hierarchy
        (r1_core, r1_prov),
        (r1_prov, r1_leaf),
        // ISD 2 hierarchy
        (r2_core, r2_prov),
        (r2_prov, r2_leaf),
        // Inter-ISD core link
        (r1_core, r2_core),
    ];
    net.add_links_from(links)?;

    // SCION links
    net.add_scion_link(r1_core, r1_prov, ScionLinkType::Child)?;
    net.add_scion_link(r1_prov, r1_leaf, ScionLinkType::Child)?;

    net.add_scion_link(r2_core, r2_prov, ScionLinkType::Child)?;
    net.add_scion_link(r2_prov, r2_leaf, ScionLinkType::Child)?;

    net.add_scion_link(r1_core, r2_core, ScionLinkType::Core)?;

    let topology = Topology {
        isd1_core,
        isd1_provider,
        isd1_leaf,
        isd2_core,
        isd2_provider,
        isd2_leaf,
        all_ases: vec![isd1_core, isd1_provider, isd1_leaf, isd2_core, isd2_provider, isd2_leaf],
        core_ases: vec![isd1_core, isd2_core],
    };

    Ok((net, topology))
}

fn run_beaconing(mut net: ScionNetwork, topology: &Topology) -> Result<ScionNetwork, NetworkError> {
    // Core beaconing
    for _ in 0..topology.core_ases.len() {
        net.propagate_core_batch(&topology.core_ases)?;
    }

    // Intra-ISD beaconing
    let mut current_level = topology.core_ases.clone();
    while !current_level.is_empty() {
        let next_level = net.propagate_intra_isd_batch(&current_level)?;
        current_level = next_level.into_iter().collect();
    }

    // Segment registration
    net.register_segments_batch(&topology.all_ases)?;

    Ok(net)
}
