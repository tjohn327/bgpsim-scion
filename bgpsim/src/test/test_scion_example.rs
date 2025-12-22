// SCION Multi-ISD Topology Example/Test
//
// This test demonstrates a realistic SCION topology with:
// - 3 ISDs (Isolation Domains)
// - 2 core ASes per ISD
// - 1 intermediate (provider) AS per ISD
// - 2 leaf (customer) ASes per ISD
// - All link types: Core, Parent-Child, and Peering
// - Full beaconing and path segment construction
// - Path lookup between various source/destination pairs

use crate::event::{BasicEventQueue, Event};
use crate::network::Network;
use crate::scion::*;
use crate::types::{RouterId, SimplePrefix};
use std::collections::HashMap;

#[test]
fn test_multi_isd_topology() {
    println!("\n=== SCION Multi-ISD Topology Test ===\n");

    // Create network
    let mut net: Network<SimplePrefix, BasicEventQueue<SimplePrefix>> = Network::default();

    println!("Step 1: Building topology...");
    let topology = build_topology(&mut net);
    println!("  ✓ Created {} routers", net.num_routers());
    println!("  ✓ Created {} ASes across 3 ISDs", topology.all_ases.len());
    println!();

    println!("Step 2: Running beaconing process...");
    run_beaconing(&mut net, &topology);
    println!("  ✓ Beaconing complete");
    print_segment_stats(&net, &topology);
    println!();

    println!("Step 3: Querying paths between various AS pairs...");
    let success_count = query_and_verify_paths(&net, &topology);
    println!();

    println!("=== Test Complete ===\n");

    // Assert that all path queries succeeded
    assert_eq!(success_count, 8, "All path queries should succeed");
}

/// Topology structure holding all routers and ASes
struct Topology {
    // ISD 1
    isd1_core1: IsdAs,
    isd1_core2: IsdAs,
    isd1_intermediate: IsdAs,
    isd1_leaf1: IsdAs,
    isd1_leaf2: IsdAs,

    // ISD 2
    isd2_core1: IsdAs,
    isd2_core2: IsdAs,
    isd2_intermediate: IsdAs,
    isd2_leaf1: IsdAs,
    isd2_leaf2: IsdAs,

    // ISD 3
    isd3_core1: IsdAs,
    isd3_core2: IsdAs,
    isd3_intermediate: IsdAs,
    isd3_leaf1: IsdAs,
    isd3_leaf2: IsdAs,

    // All ASes for iteration
    all_ases: Vec<IsdAs>,
    core_ases: Vec<IsdAs>,
}

/// Build the complete SCION topology
fn build_topology(net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>>) -> Topology {
    // Define AS identifiers
    let isd1_core1 = IsdAs::new(1, 100);
    let isd1_core2 = IsdAs::new(1, 101);
    let isd1_intermediate = IsdAs::new(1, 200);
    let isd1_leaf1 = IsdAs::new(1, 300);
    let isd1_leaf2 = IsdAs::new(1, 301);

    let isd2_core1 = IsdAs::new(2, 100);
    let isd2_core2 = IsdAs::new(2, 101);
    let isd2_intermediate = IsdAs::new(2, 200);
    let isd2_leaf1 = IsdAs::new(2, 300);
    let isd2_leaf2 = IsdAs::new(2, 301);

    let isd3_core1 = IsdAs::new(3, 100);
    let isd3_core2 = IsdAs::new(3, 101);
    let isd3_intermediate = IsdAs::new(3, 200);
    let isd3_leaf1 = IsdAs::new(3, 300);
    let isd3_leaf2 = IsdAs::new(3, 301);

    // Store router IDs for each AS
    let mut as_routers: HashMap<IsdAs, Vec<RouterId>> = HashMap::new();

    // Create core ASes (4 border routers each)
    for &core_as in &[
        isd1_core1,
        isd1_core2,
        isd2_core1,
        isd2_core2,
        isd3_core1,
        isd3_core2,
    ] {
        let routers = create_as(net, core_as, 4, true);
        as_routers.insert(core_as, routers);
    }

    // Create intermediate ASes (2 border routers each)
    for &intermediate_as in &[isd1_intermediate, isd2_intermediate, isd3_intermediate] {
        let routers = create_as(net, intermediate_as, 2, false);
        as_routers.insert(intermediate_as, routers);
    }

    // Create leaf ASes (1 border router each)
    for &leaf_as in &[
        isd1_leaf1,
        isd1_leaf2,
        isd2_leaf1,
        isd2_leaf2,
        isd3_leaf1,
        isd3_leaf2,
    ] {
        let routers = create_as(net, leaf_as, 1, false);
        as_routers.insert(leaf_as, routers);
    }

    // Add inter-AS links

    // ISD 1 internal links
    add_core_link(net, &as_routers, isd1_core1, isd1_core2, 0, 1);

    add_child_link(net, &as_routers, isd1_core1, isd1_intermediate, 2, 0);
    add_child_link(net, &as_routers, isd1_core2, isd1_intermediate, 2, 1);

    add_child_link(net, &as_routers, isd1_intermediate, isd1_leaf1, 0, 0);
    add_child_link(net, &as_routers, isd1_intermediate, isd1_leaf2, 1, 0);

    // ISD 2 internal links
    add_core_link(net, &as_routers, isd2_core1, isd2_core2, 0, 1);

    add_child_link(net, &as_routers, isd2_core1, isd2_intermediate, 2, 0);
    add_child_link(net, &as_routers, isd2_core2, isd2_intermediate, 2, 1);

    add_child_link(net, &as_routers, isd2_intermediate, isd2_leaf1, 0, 0);
    add_child_link(net, &as_routers, isd2_intermediate, isd2_leaf2, 1, 0);

    // ISD 3 internal links
    add_core_link(net, &as_routers, isd3_core1, isd3_core2, 0, 1);

    add_child_link(net, &as_routers, isd3_core1, isd3_intermediate, 2, 0);
    add_child_link(net, &as_routers, isd3_core2, isd3_intermediate, 2, 1);

    add_child_link(net, &as_routers, isd3_intermediate, isd3_leaf1, 0, 0);
    add_child_link(net, &as_routers, isd3_intermediate, isd3_leaf2, 1, 0);

    // Inter-ISD core links (forming a mesh between cores)
    // ISD 1 <-> ISD 2
    add_core_link(net, &as_routers, isd1_core1, isd2_core1, 1, 1);
    add_core_link(net, &as_routers, isd1_core2, isd2_core2, 1, 1);

    // ISD 2 <-> ISD 3
    add_core_link(net, &as_routers, isd2_core1, isd3_core1, 2, 2);
    add_core_link(net, &as_routers, isd2_core2, isd3_core2, 2, 2);

    // ISD 3 <-> ISD 1
    add_core_link(net, &as_routers, isd3_core1, isd1_core1, 1, 3);
    add_core_link(net, &as_routers, isd3_core2, isd1_core2, 1, 3);

    // Peering links between intermediate ASes
    add_peer_link(net, &as_routers, isd1_intermediate, isd2_intermediate, 0, 0);
    add_peer_link(net, &as_routers, isd2_intermediate, isd3_intermediate, 1, 1);

    Topology {
        isd1_core1,
        isd1_core2,
        isd1_intermediate,
        isd1_leaf1,
        isd1_leaf2,
        isd2_core1,
        isd2_core2,
        isd2_intermediate,
        isd2_leaf1,
        isd2_leaf2,
        isd3_core1,
        isd3_core2,
        isd3_intermediate,
        isd3_leaf1,
        isd3_leaf2,
        all_ases: vec![
            isd1_core1,
            isd1_core2,
            isd1_intermediate,
            isd1_leaf1,
            isd1_leaf2,
            isd2_core1,
            isd2_core2,
            isd2_intermediate,
            isd2_leaf1,
            isd2_leaf2,
            isd3_core1,
            isd3_core2,
            isd3_intermediate,
            isd3_leaf1,
            isd3_leaf2,
        ],
        core_ases: vec![
            isd1_core1,
            isd1_core2,
            isd2_core1,
            isd2_core2,
            isd3_core1,
            isd3_core2,
        ],
    }
}

/// Create an AS with specified number of border routers
fn create_as(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>>,
    isd_as: IsdAs,
    num_border_routers: usize,
    is_core: bool,
) -> Vec<RouterId> {
    let mut routers = Vec::new();

    // Create border routers
    for i in 0..num_border_routers {
        let name = format!("{}_{}", isd_as, i);
        let asn = isd_as.asn.0;
        let router = net.add_router(&name, asn);
        routers.push(router);
    }

    // Enable SCION on all border routers
    // First call creates control service, subsequent calls register border routers
    for &router in &routers {
        net.enable_scion_router(router, isd_as, is_core).unwrap();
    }

    // Add internal links between border routers (full mesh)
    for i in 0..routers.len() {
        for j in (i + 1)..routers.len() {
            net.add_link(routers[i], routers[j]).unwrap();
        }
    }

    routers
}

/// Add a core link between two ASes
fn add_core_link(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>>,
    as_routers: &HashMap<IsdAs, Vec<RouterId>>,
    as1: IsdAs,
    as2: IsdAs,
    router1_idx: usize,
    router2_idx: usize,
) {
    let r1 = as_routers[&as1][router1_idx];
    let r2 = as_routers[&as2][router2_idx];

    net.add_link(r1, r2).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();
}

/// Add a parent-child link
fn add_child_link(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>>,
    as_routers: &HashMap<IsdAs, Vec<RouterId>>,
    parent: IsdAs,
    child: IsdAs,
    parent_router_idx: usize,
    child_router_idx: usize,
) {
    let r_parent = as_routers[&parent][parent_router_idx];
    let r_child = as_routers[&child][child_router_idx];

    net.add_link(r_parent, r_child).unwrap();
    net.add_scion_link(r_parent, r_child, ScionLinkType::Child)
        .unwrap();
}

/// Add a peering link
fn add_peer_link(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>>,
    as_routers: &HashMap<IsdAs, Vec<RouterId>>,
    as1: IsdAs,
    as2: IsdAs,
    router1_idx: usize,
    router2_idx: usize,
) {
    let r1 = as_routers[&as1][router1_idx];
    let r2 = as_routers[&as2][router2_idx];

    net.add_link(r1, r2).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Peer).unwrap();
}

/// Run the beaconing process
fn run_beaconing(net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>>, topology: &Topology) {
    // Phase 1: Core beaconing (cores exchange beacons)
    println!("  Phase 1: Core beaconing...");

    // Run multiple rounds to ensure full propagation through the core mesh
    for _round in 0..topology.core_ases.len() {
        let mut all_beacon_events = Vec::new();

        // First, all cores originate/propagate (beacon stores start empty in round 0)
        for &core_as in &topology.core_ases {
            let event = ScionEvent::BeaconTimeout {
                interval_type: BeaconIntervalType::Core,
            };
            let scion_event = Event::scion((), core_as, core_as, event);
            let (_, events) = net.handle_scion_event(scion_event).unwrap();
            all_beacon_events.extend(events);
        }

        // Then, process all received beacons (ensures all cores originate before any receive)
        for event in all_beacon_events {
            net.handle_scion_event(event).unwrap();
        }
    }

    // Cores register core segments
    println!("  Core beacon stores before registration:");
    for &core_as in &topology.core_ases {
        let cs = &net.scion_services[&core_as];
        let beacon_count = cs.beacon_store.get_all().count();
        println!("    {}: {} PCBs in beacon store", core_as, beacon_count);
    }

    for &core_as in &topology.core_ases {
        let event = Event::scion((), core_as, core_as, ScionEvent::RegistrationTimeout);
        net.handle_scion_event(event).unwrap();
    }

    // Phase 2: Intra-ISD beaconing (cores propagate to children)
    println!("  Phase 2: Intra-ISD beaconing...");

    // Run multiple rounds to propagate down the hierarchy (core -> intermediate -> leaf)
    // 3 levels deep requires at least 3 rounds
    for round in 0..3 {
        // All non-core ASes that might have received beacons
        for &isd_as in &topology.all_ases {
            let cs = &net.scion_services[&isd_as];
            if cs.beacon_store.get_all().count() > 0 || cs.is_core {
                let event = ScionEvent::BeaconTimeout {
                    interval_type: BeaconIntervalType::IntraIsd,
                };
                let scion_event = Event::scion((), isd_as, isd_as, event);
                let (_, events) = net.handle_scion_event(scion_event).unwrap();

                // Process beacons at children
                for event in events {
                    net.handle_scion_event(event).unwrap();
                }
            }
        }
    }

    // Phase 3: Path segment registration
    println!("  Phase 3: Segment registration...");

    // Non-core ASes register segments
    for &isd_as in &topology.all_ases {
        let cs = &net.scion_services[&isd_as];
        if !cs.is_core {
            let event = Event::scion((), isd_as, isd_as, ScionEvent::RegistrationTimeout);
            let (_, events) = net.handle_scion_event(event).unwrap();

            // Process down segment registrations at cores
            for event in events {
                net.handle_scion_event(event).unwrap();
            }
        }
    }
}

/// Print statistics about registered segments
fn print_segment_stats(
    net: &Network<SimplePrefix, BasicEventQueue<SimplePrefix>>,
    topology: &Topology,
) {
    println!("\n  Segment Statistics:");

    for &isd_as in &topology.all_ases {
        let cs = &net.scion_services[&isd_as];
        let (up, down, core) = cs.path_db.segment_counts();

        if up + down + core > 0 {
            println!(
                "    {} ({}): up={}, down={}, core={}",
                isd_as,
                if cs.is_core { "core" } else { "non-core" },
                up,
                down,
                core
            );
        }
    }
}

/// Build a global path database combining segments from all ASes
/// This simulates a centralized path segment service
fn build_global_path_db(
    net: &Network<SimplePrefix, BasicEventQueue<SimplePrefix>>,
    topology: &Topology,
) -> PathDatabase {
    let mut global_db = PathDatabase::new();

    // Collect all segments from all ASes
    for &isd_as in &topology.all_ases {
        let cs = &net.scion_services[&isd_as];
        for segment in cs.path_db.get_all() {
            global_db.add_segment(segment);
        }
    }

    global_db
}

/// Query paths and verify they work correctly
fn query_and_verify_paths(
    net: &Network<SimplePrefix, BasicEventQueue<SimplePrefix>>,
    topology: &Topology,
) -> usize {
    // Build global path database (simulates path segment service)
    let global_db = build_global_path_db(net, topology);
    println!("  Global path database: {} total segments\n", global_db.get_all().len());
    let test_cases = vec![
        // Intra-ISD paths
        (
            topology.isd1_leaf1,
            topology.isd1_leaf2,
            "Intra-ISD (same ISD, different leaves)",
        ),
        (
            topology.isd2_leaf1,
            topology.isd2_leaf2,
            "Intra-ISD (same ISD, different leaves)",
        ),
        // Inter-ISD paths
        (
            topology.isd1_leaf1,
            topology.isd2_leaf1,
            "Inter-ISD (ISD 1 -> ISD 2)",
        ),
        (
            topology.isd2_leaf1,
            topology.isd3_leaf1,
            "Inter-ISD (ISD 2 -> ISD 3)",
        ),
        (
            topology.isd3_leaf1,
            topology.isd1_leaf1,
            "Inter-ISD (ISD 3 -> ISD 1)",
        ),
        // Core to non-core
        (topology.isd1_core1, topology.isd1_leaf1, "Core to leaf"),
        // Non-core to core
        (topology.isd2_leaf1, topology.isd2_core1, "Leaf to core"),
        // Core to core
        (topology.isd1_core1, topology.isd2_core1, "Core to core"),
    ];

    let mut success_count = 0;
    let mut total_paths = 0;

    for &(src, dst, description) in &test_cases {
        println!("\n  Test: {} -> {} ({})", src, dst, description);

        // Determine if source and destination are core ASes
        let src_is_core = net.scion_services[&src].is_core;
        let dst_is_core = net.scion_services[&dst].is_core;

        // Build path query using global database
        let query = PathQuery {
            src,
            dst,
            max_paths: 10,
            allow_peering: true,
        };

        let result = construct_paths_with_peering(&query, &global_db, src_is_core, dst_is_core);

        if result.is_success() {
            let num_paths = result.paths.len();
            total_paths += num_paths;

            println!("    ✓ Found {} path(s)", num_paths);

            // Show details of first path
            if let Some(path) = result.paths.first() {
                println!("      Segments: {}", path.segment_count());
                println!("      Total length: {} hops", path.total_length);
                println!(
                    "      Uses peering: {}",
                    if path.uses_peering() { "Yes" } else { "No" }
                );

                // Validate path
                match path.validate() {
                    Ok(_) => {
                        println!("      Validation: ✓ Pass");
                        success_count += 1;
                    }
                    Err(e) => {
                        println!("      Validation: ✗ Failed - {}", e);
                    }
                }

                // Show AS path
                let as_path = path.as_path();
                if as_path.len() <= 6 {
                    println!(
                        "      AS path: {}",
                        as_path
                            .iter()
                            .map(|a| a.to_string())
                            .collect::<Vec<_>>()
                            .join(" -> ")
                    );
                } else {
                    println!(
                        "      AS path: {} -> ... -> {} ({} ASes)",
                        as_path[0],
                        as_path[as_path.len() - 1],
                        as_path.len()
                    );
                }

                // Check for peering shortcut
                if let Some(ref shortcut) = path.peering_shortcut {
                    println!(
                        "      Peering shortcut: {} ⇄ {}",
                        shortcut.up_side_as, shortcut.down_side_as
                    );
                }
            }
        } else {
            println!("    ✗ No path found");
            if let Some(err) = &result.error {
                println!("      Error: {}", err);
            }
        }
    }

    println!("\n  Summary:");
    println!(
        "    {} / {} queries successful",
        success_count,
        test_cases.len()
    );
    println!("    {} total paths discovered", total_paths);

    if success_count == test_cases.len() {
        println!("    ✓ All path queries validated successfully!");
    }

    success_count
}
