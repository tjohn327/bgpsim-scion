// SPDX-License-Identifier: GPL-2.0-or-later
//
// Debug Scalability Measurement - Minimal test case
//
// Simplest possible multi-ISD setup to debug beaconing

use bgpsim::event::BasicEventQueue;
use bgpsim::ospf::GlobalOspf;
use bgpsim::prelude::*;
use bgpsim::scion::{IsdAs, IsdNumber, ScionLinkType};
use bgpsim::types::SimplePrefix;

fn main() -> Result<(), NetworkError> {
    println!("=== Debug: Minimal Multi-ISD Setup ===\n");

    let mut net: Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf> =
        Network::default();

    // ISD 1: Core1 -> Leaf1
    let core1 = net.add_router("Core1", ASN(110));
    let leaf1 = net.add_router("Leaf1", ASN(111));

    // ISD 2: Core2 -> Leaf2
    let core2 = net.add_router("Core2", ASN(210));
    let leaf2 = net.add_router("Leaf2", ASN(211));

    println!("Created 4 routers");
    println!("  Core1: {:?}", core1);
    println!("  Leaf1: {:?}", leaf1);
    println!("  Core2: {:?}", core2);
    println!("  Leaf2: {:?}", leaf2);

    // Add links BEFORE enabling SCION
    net.add_link(core1, leaf1)?;
    net.add_link(core2, leaf2)?;
    net.add_link(core1, core2)?; // Inter-ISD core link
    println!("\nAdded 3 links");

    // Enable SCION
    net.enable_scion(core1, IsdAs::new(1, 110u64), true)?;
    net.enable_scion(leaf1, IsdAs::new(1, 111u64), false)?;
    net.enable_scion(core2, IsdAs::new(2, 210u64), true)?;
    net.enable_scion(leaf2, IsdAs::new(2, 211u64), false)?;
    println!("Enabled SCION on all routers");

    // Configure SCION links
    net.configure_scion_link(core1, leaf1, ScionLinkType::ParentChild)?;
    net.configure_scion_link(core2, leaf2, ScionLinkType::ParentChild)?;
    net.configure_scion_link(core1, core2, ScionLinkType::Core)?;
    println!("Configured SCION links\n");

    // Debug: Check SCION state
    for router in [core1, leaf1, core2, leaf2] {
        let r = net.get_router(router)?;
        if let Some(scion) = r.scion() {
            println!(
                "{}: ISD-AS {}, is_core={}",
                r.name(),
                scion.isd_as,
                scion.is_core
            );
        }
    }

    // Core beaconing
    println!("\n=== Core Beaconing ===");
    let core_pcbs = net.scion_core_beaconing(1000)?;
    println!("Core PCBs created: {}", core_pcbs);

    if core_pcbs == 0 {
        println!("ERROR: No core PCBs created!");
        return Ok(());
    }

    // Intra-ISD beaconing
    println!("\n=== Intra-ISD Beaconing ===");
    let intra_pcbs = net.scion_intra_isd_beaconing(1001, 50)?;
    println!("Intra PCBs propagated: {}", intra_pcbs);

    // Registration
    println!("\n=== Registration ===");
    let (up, down, core) = net.scion_registration_round(50)?;
    println!(
        "Registered: {} up, {} down, {} core segments",
        up, down, core
    );

    // Path lookup
    println!("\n=== Path Lookup ===");
    let paths = net.scion_lookup_paths(leaf1, leaf2)?;
    println!("Leaf1 -> Leaf2: {} paths found", paths.len());
    for (i, path) in paths.iter().enumerate() {
        println!("  Path {}: {:?}", i + 1, path.as_path);
    }

    if paths.is_empty() {
        println!("\nERROR: No paths found!");
    } else {
        println!("\n✅ SUCCESS: Multi-ISD paths work!");
    }

    Ok(())
}
