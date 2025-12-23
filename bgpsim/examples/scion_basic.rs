//! Basic SCION Example
//!
//! This example demonstrates a simple SCION topology with batch simulation.
//! It shows the fundamental concepts:
//! - Creating SCION-enabled routers and ASes
//! - Adding SCION links (Core, Child, Peer)
//! - Running beaconing to propagate Path Construction Beacons (PCBs)
//! - Querying paths between source and destination ASes
//!
//! Topology:
//! ```text
//!     ┌───────┐  core  ┌───────┐
//!     │ 1-100 │◄──────►│ 1-101 │
//!     │ CORE  │        │ CORE  │
//!     └───┬───┘        └───┬───┘
//!         │                │
//!      parent           parent
//!         │                │
//!         └──────┬─────────┘
//!                ▼
//!            ┌───────┐
//!            │ 1-200 │
//!            │ LEAF  │
//!            └───────┘
//! ```
//!
//! Run with: cargo run --example scion_basic --all-features

use bgpsim::event::BasicEventQueue;
use bgpsim::network::Network;
use bgpsim::ospf::MinimalOspf;
use bgpsim::scion::*;
use bgpsim::types::{NetworkError, SimplePrefix};

// Type alias for SCION network
type ScionNetwork = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, MinimalOspf>;

fn main() -> Result<(), NetworkError> {
    println!("=== SCION Basic Example ===\n");

    // Step 1: Create network
    let mut net: ScionNetwork = Network::default();

    // Step 2: Define AS identifiers (ISD-ASN format)
    let core1 = IsdAs::new(1, 100); // ISD 1, ASN 100 (Core)
    let core2 = IsdAs::new(1, 101); // ISD 1, ASN 101 (Core)
    let leaf = IsdAs::new(1, 200);  // ISD 1, ASN 200 (Leaf)

    // Step 3: Create routers and enable SCION
    // Core AS 1-100: one border router
    let r_core1 = net.add_router("core1_br0", core1.asn.0);
    net.enable_scion_router(r_core1, core1, true)?; // true = is_core

    // Core AS 1-101: one border router
    let r_core2 = net.add_router("core2_br0", core2.asn.0);
    net.enable_scion_router(r_core2, core2, true)?;

    // Leaf AS 1-200: one border router
    let r_leaf = net.add_router("leaf_br0", leaf.asn.0);
    net.enable_scion_router(r_leaf, leaf, false)?; // false = non-core

    // Step 4: Add physical links
    net.add_link(r_core1, r_core2)?;
    net.add_link(r_core1, r_leaf)?;
    net.add_link(r_core2, r_leaf)?;

    // Step 5: Add SCION link metadata
    net.add_scion_link(r_core1, r_core2, ScionLinkType::Core)?;  // Core-to-Core
    net.add_scion_link(r_core1, r_leaf, ScionLinkType::Child)?;  // Parent-Child
    net.add_scion_link(r_core2, r_leaf, ScionLinkType::Child)?;  // Parent-Child

    println!("Topology created:");
    println!("  - 2 Core ASes: {}, {}", core1, core2);
    println!("  - 1 Leaf AS: {}", leaf);
    println!("  - 3 SCION links\n");

    // Step 6: Run beaconing (batch mode)
    println!("Running beaconing...");

    let core_ases = vec![core1, core2];
    let all_ases = vec![core1, core2, leaf];

    // Phase 1: Core beaconing
    for _ in 0..core_ases.len() {
        net.propagate_core_batch(&core_ases)?;
    }

    // Phase 2: Intra-ISD beaconing (cores propagate to children)
    let mut current_level = core_ases.clone();
    while !current_level.is_empty() {
        let next_level = net.propagate_intra_isd_batch(&current_level)?;
        current_level = next_level.into_iter().collect();
    }

    // Phase 3: Segment registration
    net.register_segments_batch(&all_ases)?;
    println!("  ✓ Beaconing complete\n");

    // Step 7: Print segment statistics
    println!("Path segments registered:");
    for &isd_as in &all_ases {
        let (up, down, core) = net.get_scion_segment_counts(&isd_as).unwrap();
        let is_core = net.is_scion_core(&isd_as).unwrap();
        println!(
            "  {} ({}): up={}, down={}, core={}",
            isd_as,
            if is_core { "core" } else { "leaf" },
            up, down, core
        );
    }
    println!();

    // Step 8: Build global path database and query paths
    let global_db = net.build_global_path_db(&all_ases)?;
    println!("Global path database: {} total segments\n", global_db.get_all().len());

    // Query: Leaf -> Core
    let query = PathQuery {
        src: leaf,
        dst: core1,
        max_paths: 10,
        allow_peering: true,
    };

    let result = construct_paths_with_peering(
        &query,
        &global_db,
        net.is_scion_core(&leaf).unwrap(),
        net.is_scion_core(&core1).unwrap(),
    );

    println!("Path query: {} -> {}", leaf, core1);
    if result.is_success() {
        println!("  ✓ Found {} path(s)", result.paths.len());
        for (i, path) in result.paths.iter().enumerate() {
            let as_path: Vec<String> = path.as_path().iter().map(|a| a.to_string()).collect();
            println!("  Path {}: {}", i + 1, as_path.join(" -> "));
        }
    } else {
        println!("  ✗ No paths found");
    }

    println!("\n=== Example Complete ===");
    Ok(())
}
