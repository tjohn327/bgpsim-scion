// SPDX-License-Identifier: GPL-2.0-or-later
//
// Check for duplicate paths in smaller topologies

use bgpsim::{
    event::BasicEventQueue,
    ospf::GlobalOspf,
    prelude::*,
    scion::{IsdAs, ScionSimulationMode},
    types::SimplePrefix,
};

fn main() -> Result<(), NetworkError> {
    let mut net: Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf> = Network::new();

    // Create a simple 2-ISD topology with multiple parents
    // ISD 1: AS1 (core), AS10 (child of AS1), AS20 (child of AS1)
    // ISD 2: AS2 (core), AS30 (child of AS2)

    // ISD 1
    let as1 = IsdAs::new(1, 1);
    let as10 = IsdAs::new(1, 10);
    let as20 = IsdAs::new(1, 20);

    // ISD 2
    let as2 = IsdAs::new(2, 2);
    let as30 = IsdAs::new(2, 30);

    // Create routers
    let r1 = net.add_router("r1");
    let r10a = net.add_router("r10a");
    let r10b = net.add_router("r10b");
    let r20a = net.add_router("r20a");
    let r20b = net.add_router("r20b");
    let r2 = net.add_router("r2");
    let r30a = net.add_router("r30a");
    let r30b = net.add_router("r30b");

    // Enable SCION
    net.set_scion_simulation_mode(ScionSimulationMode::Static);
    net.enable_scion(r1, as1, true)?;
    net.enable_scion(r10a, as10, false)?;
    net.enable_scion(r10b, as10, false)?;
    net.enable_scion(r20a, as20, false)?;
    net.enable_scion(r20b, as20, false)?;
    net.enable_scion(r2, as2, true)?;
    net.enable_scion(r30a, as30, false)?;
    net.enable_scion(r30b, as30, false)?;

    // Create links: parent-child
    net.add_link(r1, r10a);
    net.add_link(r1, r10b);
    net.add_link(r1, r20a);
    net.add_link(r1, r20b);
    net.add_link(r2, r30a);
    net.add_link(r2, r30b);

    // Core link
    net.add_link(r1, r2);

    // Configure SCION links
    net.configure_scion_link_as_level(r1, r10a, ScionLinkType::ParentChild, 1500)?;
    net.configure_scion_link_as_level(r1, r10b, ScionLinkType::ParentChild, 1500)?;
    net.configure_scion_link_as_level(r1, r20a, ScionLinkType::ParentChild, 1500)?;
    net.configure_scion_link_as_level(r1, r20b, ScionLinkType::ParentChild, 1500)?;
    net.configure_scion_link_as_level(r2, r30a, ScionLinkType::ParentChild, 1500)?;
    net.configure_scion_link_as_level(r2, r30b, ScionLinkType::ParentChild, 1500)?;
    net.configure_scion_link_as_level(r1, r2, ScionLinkType::Core, 1500)?;

    // Run beaconing
    net.scion_start_beaconing(1000)?;
    net.scion_converge()?;

    // Registration
    let limit = net.scion_simulation_mode().up_down_segment_limit();
    net.scion_registration_round(limit)?;

    // Check paths from AS10 to AS30
    println!("=== Paths from ISD1-AS10 to ISD2-AS30 ===");
    let paths = net.scion_lookup_paths(as10, as30)?;
    println!("Total paths found: {}", paths.len());

    // Check for duplicates by comparing path strings
    let mut path_strings: Vec<String> = paths.iter().map(|p| format!("{:?}", p)).collect();
    path_strings.sort();

    let unique_paths: Vec<&String> = path_strings.iter().collect();
    let total = path_strings.len();
    let unique = unique_paths.len();

    println!("Total paths: {}", total);
    println!("Unique paths: {}", unique);
    println!("Duplicates: {}", total - unique);

    if total != unique {
        println!("\n=== Duplicate paths ===");
        for (i, path) in path_strings.iter().enumerate() {
            if i > 0 && path == &path_strings[i - 1] {
                println!("Duplicate: {}", path);
            }
        }
    }

    // Show all paths
    println!("\n=== All paths ===");
    for (i, path) in paths.iter().enumerate() {
        println!("Path {}: {:?}", i + 1, path);
    }

    // Check segment counts
    println!("\n=== Segment counts ===");
    if let Some(as10_ref) = net.get_scion_as(&as10) {
        let up_segs = as10_ref
            .control_service
            .path_database
            .lookup_up_segments(&as1);
        println!("AS10 up-segments to AS1: {}", up_segs.len());
        for seg in up_segs {
            println!("  Up-seg: {:?}", seg);
        }
    }

    if let Some(core_ref) = net.get_scion_as(&as2) {
        let down_segs = core_ref.control_service.lookup_down_segments_to(&as30);
        println!("AS2 (core) down-segments to AS30: {}", down_segs.len());
        for seg in down_segs {
            println!("  Down-seg: {:?}", seg);
        }
    }

    Ok(())
}
