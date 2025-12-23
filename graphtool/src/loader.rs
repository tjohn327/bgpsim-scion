//! Loader for graph-tool graph JSON files.
//!
//! Supports both plain JSON and gzip-compressed JSON files.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use flate2::read::GzDecoder;

use crate::types::GtGraph;
use crate::GraphToolError;

/// Load a graph from a JSON file.
///
/// Automatically detects gzip compression from file extension.
///
/// # Arguments
/// * `path` - Path to the JSON or JSON.gz file
///
/// # Returns
/// The loaded graph data
///
/// # Example
/// ```ignore
/// let graph = load_graph("graphs/selectcast_ases_500.json")?;
/// println!("Loaded {} nodes, {} edges", graph.node_count(), graph.edge_count());
/// ```
pub fn load_graph<P: AsRef<Path>>(path: P) -> Result<GtGraph, GraphToolError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|e| GraphToolError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    let is_gzipped = path
        .extension()
        .map(|ext| ext == "gz")
        .unwrap_or(false);

    if is_gzipped {
        load_from_gzip(file, path)
    } else {
        load_from_json(file, path)
    }
}

fn load_from_json(file: File, path: &Path) -> Result<GtGraph, GraphToolError> {
    let reader = BufReader::with_capacity(1024 * 1024, file); // 1MB buffer for large files
    serde_json::from_reader(reader).map_err(|e| GraphToolError::Parse {
        path: path.display().to_string(),
        source: e,
    })
}

fn load_from_gzip(file: File, path: &Path) -> Result<GtGraph, GraphToolError> {
    let decoder = GzDecoder::new(file);
    let reader = BufReader::with_capacity(1024 * 1024, decoder); // 1MB buffer
    serde_json::from_reader(reader).map_err(|e| GraphToolError::Parse {
        path: path.display().to_string(),
        source: e,
    })
}

/// Load a graph from a JSON string.
pub fn load_graph_from_str(json: &str) -> Result<GtGraph, GraphToolError> {
    serde_json::from_str(json).map_err(|e| GraphToolError::Parse {
        path: "<string>".to_string(),
        source: e,
    })
}

/// Load a graph from a reader.
pub fn load_graph_from_reader<R: Read>(reader: R) -> Result<GtGraph, GraphToolError> {
    let reader = BufReader::with_capacity(1024 * 1024, reader);
    serde_json::from_reader(reader).map_err(|e| GraphToolError::Parse {
        path: "<reader>".to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_from_str() {
        let json = r#"{
            "nodes": [
                {"id": 0, "asn": 64500, "isd": 1, "border_router": true, "core": false, "interfaces": [1, 2]},
                {"id": 1, "asn": 64501, "isd": 1, "border_router": true, "core": true, "interfaces": [3]}
            ],
            "edges": [
                {"source": 0, "target": 1, "inter_as": true, "rtt_sec": 0.001, "distance_km": 100.0, "skipped_hops": 0}
            ]
        }"#;

        let graph = load_graph_from_str(json).unwrap();
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);
        assert_eq!(graph.nodes[0].asn, 64500);
        assert_eq!(graph.nodes[1].core, true);
        assert_eq!(graph.edges[0].inter_as, true);
    }

    #[test]
    fn test_unique_ases() {
        let json = r#"{
            "nodes": [
                {"id": 0, "asn": 100, "isd": 1, "border_router": false, "core": true, "interfaces": []},
                {"id": 1, "asn": 100, "isd": 1, "border_router": true, "core": true, "interfaces": []},
                {"id": 2, "asn": 200, "isd": 1, "border_router": false, "core": false, "interfaces": []},
                {"id": 3, "asn": 100, "isd": 2, "border_router": false, "core": true, "interfaces": []}
            ],
            "edges": []
        }"#;

        let graph = load_graph_from_str(json).unwrap();
        let ases = graph.unique_ases();
        assert_eq!(ases.len(), 3); // (1, 100), (1, 200), (2, 100)
    }
}
