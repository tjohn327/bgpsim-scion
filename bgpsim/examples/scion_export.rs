//! SCION Path Database Export Example
//!
//! This example demonstrates how to export the global path database to JSON
//! for analysis in Python or other tools.
//!
//! The exported JSON has this structure:
//! ```json
//! {
//!   "up_segments": [...],
//!   "down_segments": [...],
//!   "core_segments": [...]
//! }
//! ```
//!
//! Each segment contains:
//! - segment_type: "Up", "Down", or "Core"
//! - pcb: { segment_info, as_entries }
//!
//! Run with: cargo run --example scion_export --all-features

use bgpsim::event::BasicEventQueue;
use bgpsim::network::Network;
use bgpsim::ospf::MinimalOspf;
use bgpsim::scion::*;
use bgpsim::types::{NetworkError, SimplePrefix};

type ScionNetwork = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, MinimalOspf>;

fn main() -> Result<(), NetworkError> {
    println!("=== SCION Path Database Export Example ===\n");

    // Build a simple topology
    let mut net: ScionNetwork = Network::default();

    // ISD 1: core + leaf
    let core = IsdAs::new(1, 100);
    let leaf = IsdAs::new(1, 200);

    let r_core = net.add_router("core", 100);
    let r_leaf = net.add_router("leaf", 200);

    net.enable_scion_router(r_core, core, true)?;
    net.enable_scion_router(r_leaf, leaf, false)?;

    net.add_link(r_core, r_leaf)?;
    net.add_scion_link(r_core, r_leaf, ScionLinkType::Child)?;

    // Run beaconing
    let all_ases = vec![core, leaf];
    net.propagate_core_batch(&[core])?;
    let mut level = vec![core];
    while !level.is_empty() {
        level = net.propagate_intra_isd_batch(&level)?.into_iter().collect();
    }
    net.register_segments_batch(&all_ases)?;

    // Build global path database
    let global_db = net.build_global_path_db(&all_ases)?;
    println!("Built path database with {} segments\n", global_db.total_segments());

    // =====================================
    // Export to JSON string
    // =====================================
    println!("Method 1: Export to JSON string");
    println!("────────────────────────────────");

    let json = global_db.to_json().expect("Failed to serialize");
    println!("{}", json);
    println!();

    // =====================================
    // Export to file
    // =====================================
    println!("Method 2: Export to file");
    println!("────────────────────────────────");

    let path = "/tmp/scion_paths.json";
    global_db.to_json_file(path).expect("Failed to write file");
    println!("Exported to: {}\n", path);

    // =====================================
    // Load back from file
    // =====================================
    println!("Method 3: Load from file");
    println!("────────────────────────────────");

    let loaded = PathDatabaseExport::from_json_file(path).expect("Failed to load");
    println!("Loaded {} segments from file", loaded.total_segments());

    // Convert back to PathDatabase for queries
    let restored_db = loaded.into_database();
    println!("Restored database with {} segments\n", restored_db.total_segments());

    // =====================================
    // Python usage example
    // =====================================
    println!("═══════════════════════════════════════════");
    println!("Python Usage:");
    println!("═══════════════════════════════════════════");
    println!(r#"
import json

# Load the exported data
with open('/tmp/scion_paths.json') as f:
    data = json.load(f)

# Access segments by type
up_segments = data['up_segments']
down_segments = data['down_segments']
core_segments = data['core_segments']

# Example: Print all AS paths
for seg in up_segments:
    path = [e['isd_as'] for e in seg['pcb']['as_entries']]
    print(f"UP: {{' -> '.join(str(p) for p in path)}}")

# Example: Analyze hop fields
for seg in down_segments:
    for entry in seg['pcb']['as_entries']:
        isd_as = entry['isd_as']
        hop = entry['hop_entry']
        ingress = hop['ingress']['0']  # Interface ID
        egress = hop.get('egress', {{}}).get('0', '-')
        print(f"  {{isd_as}}: in={{ingress}}, out={{egress}}")
"#);

    println!("\n=== Example Complete ===");
    Ok(())
}
