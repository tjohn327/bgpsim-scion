//! SCION with GlobalOspf Example
//!
//! This example demonstrates using GlobalOspf with SCION when you need
//! accurate intra-AS routing simulation:
//!
//! - **Full SPT computation**: Shortest-path tree for each router
//! - **Accurate link weights**: Real OSPF costs between routers
//! - **Multi-router ASes**: Model complex internal topologies
//!
//! Use GlobalOspf when:
//! - Studying IGP-SCION interaction
//! - Need accurate intra-AS forwarding paths
//! - Modeling real network topologies with multiple border routers
//!
//! Trade-off: Slower than MinimalOspf for large simulations.
//!
//! Run with: cargo run --example scion_global_ospf --all-features

use bgpsim::event::BasicEventQueue;
use bgpsim::network::Network;
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::*;
use bgpsim::types::{NetworkError, SimplePrefix};
use std::time::Instant;

// GlobalOspf: Full SPT computation with real link weights
type ScionNetwork = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>;

fn main() -> Result<(), NetworkError> {
    println!("=== SCION with GlobalOspf ===\n");

    println!("GlobalOspf Characteristics:");
    println!("  • Full shortest-path tree computation");
    println!("  • Real OSPF link weights");
    println!("  • Accurate intra-AS routing");
    println!("  • Best for multi-router AS topologies\n");

    // Build topology with multiple routers per AS
    let start = Instant::now();
    let (mut net, topology) = build_multi_router_topology()?;
    let build_time = start.elapsed();

    println!("Topology built in {:?}", build_time);
    println!("  • 2 ASes with internal topology");
    println!("  • Multiple border routers per AS\n");

    // ========================================
    // Demonstrate OSPF link weights
    // ========================================
    println!("OSPF Configuration:");
    println!("─────────────────────");

    // Set link weights within the core AS
    // This affects intra-AS routing (not SCION path selection)
    net.set_link_weight(topology.core_br1, topology.core_internal, 10.0)?;
    net.set_link_weight(topology.core_internal, topology.core_br1, 10.0)?;
    net.set_link_weight(topology.core_br2, topology.core_internal, 20.0)?;
    net.set_link_weight(topology.core_internal, topology.core_br2, 20.0)?;

    println!("  Core AS internal weights:");
    println!("    BR1 <-> Internal: 10.0");
    println!("    BR2 <-> Internal: 20.0");
    println!("  (Traffic prefers BR1 path within AS)\n");

    // Run beaconing
    let start = Instant::now();
    let net = run_beaconing(net, &topology)?;
    let beacon_time = start.elapsed();
    println!("Beaconing completed in {:?}", beacon_time);

    // Build path database
    let global_db = net.build_global_path_db(&topology.all_ases)?;
    println!("Path database: {} segments\n", global_db.get_all().len());

    // Query paths
    println!("Path Queries:");
    println!("─────────────");

    let query = PathQuery {
        src: topology.leaf,
        dst: topology.core,
        max_paths: 10,
        allow_peering: true,
    };

    let src_is_core = net.is_scion_core(&topology.leaf).unwrap();
    let dst_is_core = net.is_scion_core(&topology.core).unwrap();
    let result = construct_paths_with_peering(&query, &global_db, src_is_core, dst_is_core);

    if result.is_success() {
        println!("  {} → {}: {} path(s) found", topology.leaf, topology.core, result.paths.len());

        for (i, path) in result.paths.iter().enumerate() {
            let as_path: Vec<String> = path.as_path().iter().map(|a| a.to_string()).collect();
            println!("    Path {}: {}", i + 1, as_path.join(" -> "));
        }
    }

    // ========================================
    // Explain the difference
    // ========================================
    println!("\n═══════════════════════════════════════════");
    println!("GlobalOspf vs MinimalOspf:");
    println!("═══════════════════════════════════════════");
    println!();
    println!("  GlobalOspf:");
    println!("    • Computes real shortest paths within AS");
    println!("    • Link weights affect intra-AS routing");
    println!("    • Suitable for small/medium topologies");
    println!("    • Use when internal routing matters");
    println!();
    println!("  MinimalOspf:");
    println!("    • Assumes full mesh (all routers adjacent)");
    println!("    • No SPT computation overhead");
    println!("    • ~30% faster for large topologies");
    println!("    • Use when only SCION paths matter");
    println!();
    println!("  SCION Path Selection:");
    println!("    • SCION paths are inter-AS (hop fields)");
    println!("    • Intra-AS routing is transparent to SCION");
    println!("    • GlobalOspf matters for BGP next-hop resolution");

    println!("\n=== Example Complete ===");
    Ok(())
}

#[allow(dead_code)]
struct Topology {
    core: IsdAs,
    leaf: IsdAs,
    core_br1: bgpsim::types::RouterId,
    core_br2: bgpsim::types::RouterId,
    core_internal: bgpsim::types::RouterId,
    all_ases: Vec<IsdAs>,
}

fn build_multi_router_topology() -> Result<(ScionNetwork, Topology), NetworkError> {
    let mut net: ScionNetwork = Network::default();

    let core = IsdAs::new(1, 100);
    let leaf = IsdAs::new(1, 200);

    // Core AS has 3 routers: 2 border routers + 1 internal
    let core_br1 = net.add_router("core_br1", 100);
    let core_br2 = net.add_router("core_br2", 100);
    let core_internal = net.add_router("core_internal", 100);

    // Leaf AS has 1 router
    let leaf_br = net.add_router("leaf_br", 200);

    // Enable SCION on border routers only
    // (Internal routers don't need SCION - they forward based on OSPF)
    net.enable_scion_router(core_br1, core, true)?;
    net.enable_scion_router(core_br2, core, true)?;
    net.enable_scion_router(leaf_br, leaf, false)?;

    // Internal links (core AS internal topology)
    let internal_links = vec![
        (core_br1, core_internal),
        (core_br2, core_internal),
    ];
    net.add_links_from(internal_links)?;

    // External link (core BR1 -> leaf)
    net.add_link(core_br1, leaf_br)?;

    // SCION link (parent-child)
    net.add_scion_link(core_br1, leaf_br, ScionLinkType::Child)?;

    let topology = Topology {
        core,
        leaf,
        core_br1,
        core_br2,
        core_internal,
        all_ases: vec![core, leaf],
    };

    Ok((net, topology))
}

fn run_beaconing(mut net: ScionNetwork, topology: &Topology) -> Result<ScionNetwork, NetworkError> {
    let core_ases = vec![topology.core];

    // Core beaconing (single core, so one round)
    net.propagate_core_batch(&core_ases)?;

    // Intra-ISD beaconing
    let mut current_level = core_ases;
    while !current_level.is_empty() {
        let next_level = net.propagate_intra_isd_batch(&current_level)?;
        current_level = next_level.into_iter().collect();
    }

    // Segment registration
    net.register_segments_batch(&topology.all_ases)?;

    Ok(net)
}
