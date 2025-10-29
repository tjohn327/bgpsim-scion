// SPDX-License-Identifier: GPL-2.0-or-later
//
// Simple Multi-ISD Example
//
// Demonstrates basic SCION multi-ISD setup with:
// - 2 ISDs
// - 1 Core AS per ISD
// - 2 Leaf ASes per ISD
// - Inter-ISD path lookup

use bgpsim::prelude::*;
use bgpsim::event::BasicEventQueue;
use bgpsim::types::SimplePrefix;
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::{IsdAs, ScionLinkType};

fn main() -> Result<(), NetworkError> {
    // Create network
    type Net = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>;
    let mut net = Net::default();

    println!("=== Creating 2-ISD Topology ===");
    println!("Each ISD: 1 Core + 2 Leaf ASes\n");

    // ==========================================
    // ISD 1: 1 Core + 2 Leaf
    // ==========================================

    // Core AS
    let isd1_core = net.add_router("ISD1-Core", 110);
    net.enable_scion(isd1_core, IsdAs::new(1, 110u64), true)?;  // is_core=true

    // Leaf ASes
    let isd1_leaf1 = net.add_router("ISD1-Leaf1", 111);
    let isd1_leaf2 = net.add_router("ISD1-Leaf2", 112);
    net.enable_scion(isd1_leaf1, IsdAs::new(1, 111u64), false)?;
    net.enable_scion(isd1_leaf2, IsdAs::new(1, 112u64), false)?;

    // Links: Core ↔ Leaf (ParentChild)
    net.add_link(isd1_core, isd1_leaf1)?;
    net.configure_scion_link(isd1_core, isd1_leaf1, ScionLinkType::ParentChild)?;

    net.add_link(isd1_core, isd1_leaf2)?;
    net.configure_scion_link(isd1_core, isd1_leaf2, ScionLinkType::ParentChild)?;

    println!("✓ ISD 1 created: 1-110 (core), 1-111, 1-112 (leaves)");

    // ==========================================
    // ISD 2: 1 Core + 2 Leaf
    // ==========================================

    // Core AS
    let isd2_core = net.add_router("ISD2-Core", 210);
    net.enable_scion(isd2_core, IsdAs::new(2, 210u64), true)?;

    // Leaf ASes
    let isd2_leaf1 = net.add_router("ISD2-Leaf1", 211);
    let isd2_leaf2 = net.add_router("ISD2-Leaf2", 212);
    net.enable_scion(isd2_leaf1, IsdAs::new(2, 211u64), false)?;
    net.enable_scion(isd2_leaf2, IsdAs::new(2, 212u64), false)?;

    // Links: Core ↔ Leaf
    net.add_link(isd2_core, isd2_leaf1)?;
    net.configure_scion_link(isd2_core, isd2_leaf1, ScionLinkType::ParentChild)?;

    net.add_link(isd2_core, isd2_leaf2)?;
    net.configure_scion_link(isd2_core, isd2_leaf2, ScionLinkType::ParentChild)?;

    println!("✓ ISD 2 created: 2-210 (core), 2-211, 2-212 (leaves)");

    // ==========================================
    // Inter-ISD Core Link
    // ==========================================

    net.add_link(isd1_core, isd2_core)?;
    net.configure_scion_link(isd1_core, isd2_core, ScionLinkType::Core)?;

    println!("✓ Inter-ISD core link: 1-110 ↔ 2-210\n");

    // ==========================================
    // Run Beaconing
    // ==========================================

    println!("=== Running Beaconing ===");

    // Need 2 rounds for inter-ISD propagation
    let mut timestamp = 1000u32;

    for round in 1..=2 {
        println!("\nRound {}:", round);

        // Core beaconing (creates PCBs at core ASes)
        let core_pcbs = net.scion_core_beaconing(timestamp)?;
        println!("  Core beaconing: {} PCBs", core_pcbs);
        timestamp += 1;

        // Intra-ISD beaconing (propagates to leaves)
        // Need 2 rounds: first to reach leaves, second for cross-ISD
        for sub_round in 1..=2 {
            let intra_pcbs = net.scion_intra_isd_beaconing(timestamp, 50)?;
            println!("  Intra-ISD round {}: {} PCBs", sub_round, intra_pcbs);
            timestamp += 1;
        }
    }

    // ==========================================
    // Register Segments
    // ==========================================

    println!("\n=== Registering Segments ===");
    let (up, down, core) = net.scion_registration_round(50)?;
    println!("Registered: {} up, {} down, {} core segments\n", up, down, core);

    // ==========================================
    // Path Lookup Examples
    // ==========================================

    println!("=== Path Lookup Examples ===\n");

    // 1. Intra-ISD path (same ISD)
    println!("1. Intra-ISD: ISD1-Leaf1 → ISD1-Leaf2");
    let segments = net.scion_lookup_path_segments(isd1_leaf1, isd1_leaf2)?;
    println!("   Up segments:   {}", segments.up_segments.len());
    println!("   Core segments: {}", segments.core_segments.len());
    println!("   Down segments: {}", segments.down_segments.len());

    // 2. Inter-ISD path (different ISDs)
    println!("\n2. Inter-ISD: ISD1-Leaf1 → ISD2-Leaf1");
    let segments = net.scion_lookup_path_segments(isd1_leaf1, isd2_leaf1)?;
    println!("   Up segments:   {}", segments.up_segments.len());
    println!("   Core segments: {}", segments.core_segments.len());
    println!("   Down segments: {}", segments.down_segments.len());

    // 3. Another inter-ISD path
    println!("\n3. Inter-ISD: ISD1-Leaf2 → ISD2-Leaf2");
    let segments = net.scion_lookup_path_segments(isd1_leaf2, isd2_leaf2)?;
    println!("   Up segments:   {}", segments.up_segments.len());
    println!("   Core segments: {}", segments.core_segments.len());
    println!("   Down segments: {}", segments.down_segments.len());

    // Show segment details
    if !segments.up_segments.is_empty() {
        println!("\n   Example up-segment:");
        let seg = &segments.up_segments[0];
        println!("     {} hops: {:?}", seg.as_path.len(),
                 seg.as_path.iter().map(|h| format!("{}-{}", h.isd, h.asn)).collect::<Vec<_>>());
    }

    if !segments.core_segments.is_empty() {
        println!("\n   Example core-segment:");
        let seg = &segments.core_segments[0];
        println!("     {} hops: {:?}", seg.as_path.len(),
                 seg.as_path.iter().map(|h| format!("{}-{}", h.isd, h.asn)).collect::<Vec<_>>());
    }

    if !segments.down_segments.is_empty() {
        println!("\n   Example down-segment:");
        let seg = &segments.down_segments[0];
        println!("     {} hops: {:?}", seg.as_path.len(),
                 seg.as_path.iter().map(|h| format!("{}-{}", h.isd, h.asn)).collect::<Vec<_>>());
    }

    println!("\n=== Complete ===");
    Ok(())
}
