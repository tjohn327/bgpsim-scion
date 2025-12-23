# GraphTool Graph Loader

This module provides tools to load graph-tool (.gt.gz) graphs into bgpsim for SCION simulation.

## Quick Start

### Option 1: Using Python Converter (Recommended)

1. **Install graph-tool** (Python):
   ```bash
   # Using conda (recommended)
   conda install -c conda-forge graph-tool

   # Or using pip (may not work on all platforms)
   pip install graph-tool
   ```

2. **Convert the .gt.gz file to JSON**:
   ```bash
   python3 convert_gt_to_json.py graphs/selectcast_ases_500.gt.gz graphs/selectcast_ases_500.json
   ```

3. **Run the simulation**:
   ```bash
   cargo run --example load_scion_graph -p graphtool -- graphs/selectcast_ases_500.json
   ```

### Option 2: View Graph Information

To inspect what properties a graph file contains:
```bash
python3 convert_gt_to_json.py --info graphs/selectcast_ases_500.gt.gz
```

## File Format

### Input: .gt.gz (graph-tool format)

The converter expects these vertex and edge properties:

**Vertex attributes:**
- `asn`: int - AS Number
- `isd`: int - ISD Number (default: 1)
- `border_router`: bool - Whether this is a border router (default: true)
- `core`: bool - True for all routers that belong to core ASes
- `interfaces`: vector of ints - Interface IDs

**Edge attributes:**
- `inter_as`: bool - Whether this is an inter-AS link (auto-detected if missing)
- `rtt_data`: double - Average RTT in seconds
- `distance_km`: double - Distance in kilometers
- `skipped_hops`: int - Number of skipped hops

Missing properties will use sensible defaults.

### Output: JSON intermediate format

```json
{
  "nodes": [
    {
      "id": 0,
      "asn": 64500,
      "isd": 1,
      "border_router": true,
      "core": false,
      "interfaces": [1, 2, 3]
    }
  ],
  "edges": [
    {
      "source": 0,
      "target": 1,
      "inter_as": true,
      "rtt_sec": 0.001,
      "distance_km": 100.0,
      "skipped_hops": 0
    }
  ]
}
```

## Rust API

```rust
use graphtool::{load_graph, run_beaconing, ScionNetworkBuilder};
use bgpsim::scion::{construct_paths_with_peering, IsdAs, PathQuery};

// Load the converted JSON
let graph = load_graph("graphs/selectcast_ases_500.json")?;

// Build the SCION network
let builder = ScionNetworkBuilder::new(graph);
let (mut net, stats) = builder.build()?;

println!("Built network with {} ASes, {} routers", stats.as_count, stats.router_count);

// Get AS info
let all_ases: Vec<IsdAs> = graph.unique_ases()
    .iter()
    .map(|(isd, asn)| IsdAs::new(*isd, *asn))
    .collect();

let core_ases: Vec<IsdAs> = net
    .get_scion_services()
    .iter()
    .filter_map(|(isd_as, svc)| if svc.is_core { Some(*isd_as) } else { None })
    .collect();

// Run beaconing
run_beaconing(&mut net, &core_ases, &all_ases)?;

// Build global path database
let global_db = net.build_global_path_db(&all_ases)?;

// Query paths
let query = PathQuery {
    src: IsdAs::new(1, 200),
    dst: IsdAs::new(1, 100),
    max_paths: 10,
    allow_peering: true,
};

let result = construct_paths_with_peering(
    &query,
    &global_db,
    net.is_scion_core(&query.src).unwrap_or(false),
    net.is_scion_core(&query.dst).unwrap_or(false),
);

if result.is_success() {
    println!("Found {} paths", result.paths.len());
}
```

## Performance Considerations

The loader is designed for efficiency:

1. **Batch Operations**: Links are added using `add_links_from()` for bulk insertion
2. **MinimalOspf**: Uses MinimalOspf for efficient large-scale simulation
3. **Streaming JSON**: Uses buffered readers for large files
4. **Compressed Support**: Handles .json.gz files directly

## Examples

### Load JSON Graph
```bash
cargo run --example load_scion_graph -p graphtool -- graphs/sample_test.json
```

### Load .gt.gz Native (Experimental)
```bash
cargo run --example load_gt_native -p graphtool -- graphs/selectcast_ases_500.gt.gz
```

Note: The native .gt parser is experimental and may not support all graph-tool features.

## Testing

```bash
# Run library tests
cargo test -p graphtool

# Run with sample data
cargo run --example load_scion_graph -p graphtool -- graphs/sample_test.json
```

## Troubleshooting

### "graph-tool is not installed"

Install graph-tool using conda:
```bash
conda install -c conda-forge graph-tool
```

### Missing properties

The converter will print warnings and use defaults for missing properties:
- Missing `asn`: Uses vertex ID as ASN
- Missing `isd`: Uses ISD 1
- Missing `core`: Uses false
- Missing `inter_as`: Auto-detected from ASN differences

### Large graphs

For very large graphs (>100k vertices):
1. Use compressed JSON output: `convert_gt_to_json.py input.gt.gz output.json.gz`
2. Consider running in release mode: `cargo run --release --example load_scion_graph ...`
