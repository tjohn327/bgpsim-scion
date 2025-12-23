//! Types for representing graph-tool graph data in JSON format.
//!
//! These types are used for deserializing JSON output from the Python converter.

use serde::{Deserialize, Serialize};

/// A node (vertex) from the graph-tool graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GtNode {
    /// Vertex index in the original graph
    pub id: u32,
    /// AS Number
    pub asn: u32,
    /// ISD Number
    pub isd: u16,
    /// Whether this is a border router
    pub border_router: bool,
    /// True for all routers that belong to core ASes
    pub core: bool,
    /// Interface IDs
    #[serde(default)]
    pub interfaces: Vec<u16>,
}

/// An edge from the graph-tool graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GtEdge {
    /// Source vertex index
    pub source: u32,
    /// Target vertex index
    pub target: u32,
    /// Whether this is an inter-AS link
    pub inter_as: bool,
    /// Average RTT in seconds
    #[serde(default)]
    pub rtt_sec: f64,
    /// Distance in kilometers
    #[serde(default)]
    pub distance_km: f64,
    /// Number of skipped hops
    #[serde(default)]
    pub skipped_hops: u32,
}

/// Complete graph data loaded from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GtGraph {
    /// All nodes in the graph
    pub nodes: Vec<GtNode>,
    /// All edges in the graph
    pub edges: Vec<GtEdge>,
}

impl GtGraph {
    /// Returns the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Returns unique ISD-AS pairs in the graph.
    pub fn unique_ases(&self) -> Vec<(u16, u32)> {
        let mut ases: std::collections::BTreeSet<(u16, u32)> = std::collections::BTreeSet::new();
        for node in &self.nodes {
            ases.insert((node.isd, node.asn));
        }
        ases.into_iter().collect()
    }

    /// Returns unique ISD numbers in the graph.
    pub fn unique_isds(&self) -> Vec<u16> {
        let mut isds: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
        for node in &self.nodes {
            isds.insert(node.isd);
        }
        isds.into_iter().collect()
    }
}
