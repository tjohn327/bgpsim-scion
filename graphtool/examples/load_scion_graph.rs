//! Example: Load a graph-tool graph and run SCION simulation.
//!
//! This example demonstrates the full workflow:
//! 1. Load a JSON graph (converted from .gt.gz)
//! 2. Build a SCION network
//! 3. Run beaconing
//! 4. Query paths between ASes
//!
//! # Usage
//!
//! First convert the graph:
//! ```bash
//! python3 convert_gt_to_json.py graphs/selectcast_ases_500.gt.gz graphs/selectcast_ases_500.json
//! ```
//!
//! Then run this example:
//! ```bash
//! cargo run --example load_scion_graph -- graphs/selectcast_ases_500.json
//! ```

use std::env;
use std::time::Instant;

use graphtool::{load_graph, run_beaconing_with_progress, ScionNetworkBuilder};

use bgpsim::scion::{construct_paths_with_peering, IsdAs, PathQuery};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <graph.json>", args[0]);
        eprintln!();
        eprintln!("First convert the graph-tool graph:");
        eprintln!("  python3 convert_gt_to_json.py graphs/input.gt.gz graphs/input.json");
        std::process::exit(1);
    }

    let graph_path = &args[1];
    println!("=== GraphTool SCION Simulation ===\n");

    // Step 1: Load the graph
    println!("Loading graph from {}...", graph_path);
    let start = Instant::now();
    let graph = load_graph(graph_path)?;
    println!(
        "  Loaded {} nodes, {} edges in {:?}",
        graph.node_count(),
        graph.edge_count(),
        start.elapsed()
    );

    // Print graph statistics
    let isds = graph.unique_isds();
    let ases = graph.unique_ases();
    println!("  {} ISDs, {} unique ASes", isds.len(), ases.len());
    println!();

    // Step 2: Build the SCION network
    println!("Building SCION network...");
    let start = Instant::now();
    let builder = ScionNetworkBuilder::new(graph);
    let (mut net, stats) = builder.build()?;
    println!("  Built in {:?}", start.elapsed());
    println!("  Network statistics:");
    println!("    - ISDs: {}", stats.isd_count);
    println!("    - ASes: {} ({} core)", stats.as_count, stats.core_as_count);
    println!("    - Routers: {}", stats.router_count);
    println!("    - Physical links: {}", stats.link_count);
    println!("    - SCION links: {}", stats.scion_link_count);
    println!("    - Intra-AS links: {}", stats.intra_as_link_count);
    println!();

    // Step 3: Run beaconing
    println!("Running SCION beaconing...");
    let start = Instant::now();

    // Collect AS info
    let all_ases: Vec<IsdAs> = ases
        .iter()
        .map(|(isd, asn)| IsdAs::new(*isd, *asn))
        .collect();

    // Identify core ASes (from build stats we know which are core)
    // SORT for deterministic ordering (HashMap iteration is non-deterministic)
    let mut core_ases: Vec<IsdAs> = net
        .get_scion_services()
        .iter()
        .filter_map(|(isd_as, svc)| if svc.is_core { Some(*isd_as) } else { None })
        .collect();
    core_ases.sort();

    println!("  Core ASes: {}", core_ases.len());
    run_beaconing_with_progress(&mut net, &core_ases, &all_ases, true)?;
    println!("  Beaconing completed in {:?}", start.elapsed());
    println!();

    // Step 4: Build global path database
    println!("Building global path database...");
    let start = Instant::now();
    let global_db = net.build_global_path_db(&all_ases)?;
    println!(
        "  Built in {:?} ({} segments)",
        start.elapsed(),
        global_db.get_all().len()
    );

    // Analyze segment distribution
    let segments = global_db.get_all();
    let up_count = segments.iter().filter(|s| s.segment_type.is_up()).count();
    let down_count = segments.iter().filter(|s| s.segment_type.is_down()).count();
    let core_count = segments.iter().filter(|s| s.segment_type.is_core()).count();
    println!("  Segment breakdown: {} UP, {} DOWN, {} CORE", up_count, down_count, core_count);

    // Sample segment lengths
    let mut lengths: Vec<usize> = segments.iter().map(|s| s.pcb.len()).collect();
    lengths.sort();
    if !lengths.is_empty() {
        println!("  Segment lengths: min={}, max={}, median={}",
            lengths[0],
            lengths[lengths.len()-1],
            lengths[lengths.len()/2]);
    }
    println!();

    // Step 5: Query some paths
    println!("=== Path Queries ===\n");

    // Collect test pairs - SORT for deterministic ordering
    let mut non_core_ases: Vec<IsdAs> = all_ases
        .iter()
        .filter(|a| !net.is_scion_core(a).unwrap_or(true))
        .copied()
        .collect();
    non_core_ases.sort();

    let test_cases: Vec<(IsdAs, IsdAs, &str)> = vec![
        // Non-core to core
        (non_core_ases.get(0).copied().unwrap_or(all_ases[0]),
         core_ases.get(0).copied().unwrap_or(all_ases[0]),
         "Non-core to Core"),
        // Core to non-core
        (core_ases.get(0).copied().unwrap_or(all_ases[0]),
         non_core_ases.get(0).copied().unwrap_or(all_ases[0]),
         "Core to Non-core"),
        // Non-core to non-core
        (non_core_ases.get(0).copied().unwrap_or(all_ases[0]),
         non_core_ases.get(10).copied().unwrap_or(all_ases[1]),
         "Non-core to Non-core"),
        // Core to core
        (core_ases.get(0).copied().unwrap_or(all_ases[0]),
         core_ases.get(1).copied().unwrap_or(all_ases[1]),
         "Core to Core"),
        // Random pairs
        (all_ases[all_ases.len() / 4],
         all_ases[all_ases.len() * 3 / 4],
         "Random pair 1"),
        (all_ases[all_ases.len() / 3],
         all_ases[all_ases.len() * 2 / 3],
         "Random pair 2"),
    ];

    let mut success_count = 0;
    for (src, dst, description) in &test_cases {
        if src == dst {
            continue;
        }

        let query = PathQuery {
            src: *src,
            dst: *dst,
            max_paths: 1000,  // High limit to find all paths
            allow_peering: true,
        };

        let result = construct_paths_with_peering(
            &query,
            &global_db,
            net.is_scion_core(src).unwrap_or(false),
            net.is_scion_core(dst).unwrap_or(false),
        );

        print!("{}: {} -> {} ... ", description, src, dst);
        if result.is_success() {
            let path = &result.paths[0];
            let as_path: Vec<String> = path.as_path().iter().map(|a| a.to_string()).collect();
            println!("{} paths, {} hops ({})",
                result.paths.len(),
                as_path.len(),
                as_path.join(" -> "));
            success_count += 1;
        } else {
            println!("NO PATH");
        }
    }

    println!("\n{}/{} path queries successful", success_count, test_cases.len());
    println!("\n=== Simulation Complete ===");
    Ok(())
}
