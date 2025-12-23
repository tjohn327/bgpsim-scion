//! GraphTool Graph Loader for bgpsim SCION simulation.
//!
//! This crate provides tools to load graph-tool (.gt.gz) graphs into bgpsim
//! for SCION beaconing simulation. The workflow is:
//!
//! 1. Convert .gt.gz to JSON using the Python converter script
//! 2. Load the JSON into Rust using [`load_graph`]
//! 3. Build a SCION network using [`ScionNetworkBuilder`]
//! 4. Run beaconing and query paths
//!
//! # Example
//!
//! ```ignore
//! use graphtool::{load_graph, ScionNetworkBuilder, run_beaconing};
//!
//! // Load the converted JSON
//! let graph = load_graph("graphs/selectcast_ases_500.json")?;
//!
//! // Build the SCION network
//! let builder = ScionNetworkBuilder::new(graph);
//! let (mut net, stats) = builder.build()?;
//!
//! println!("Built network with {} ASes, {} routers", stats.as_count, stats.router_count);
//!
//! // Run beaconing
//! let core_ases = vec![...];
//! let all_ases = vec![...];
//! run_beaconing(&mut net, &core_ases, &all_ases)?;
//! ```
//!
//! # Graph Format
//!
//! The input graph should have these vertex attributes:
//! - `asn`: AS Number (u32)
//! - `isd`: ISD Number (u16)
//! - `border_router`: Whether this is a border router (bool)
//! - `core`: True for routers in core ASes (bool)
//! - `interfaces`: Interface IDs (Vec<u16>)
//!
//! And these edge attributes:
//! - `inter_as`: Whether this is an inter-AS link (bool)
//! - `rtt_sec`: Average RTT in seconds (f64)
//! - `distance_km`: Distance in kilometers (f64)
//! - `skipped_hops`: Number of skipped hops (u32)

mod loader;
mod scion_builder;
mod types;

pub use loader::{load_graph, load_graph_from_reader, load_graph_from_str};
pub use scion_builder::{
    build_and_run_beaconing, run_beaconing, run_beaconing_with_progress, BuildStats, ScionNetwork, ScionNetworkBuilder,
};
pub use types::{GtEdge, GtGraph, GtNode};

use thiserror::Error;

/// Errors that can occur when loading or building from graph-tool graphs.
#[derive(Error, Debug)]
pub enum GraphToolError {
    /// IO error when reading a file.
    #[error("IO error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// JSON parsing error.
    #[error("Parse error in {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    /// Network construction error.
    #[error("Network error: {message}")]
    Network { message: String },
}
