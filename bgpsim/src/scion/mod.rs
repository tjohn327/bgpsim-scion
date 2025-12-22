//! SCION (Scalability, Control, and Isolation On Next-generation networks) integration
//!
//! This module implements SCION's beaconing protocol and path-aware routing
//! to work alongside BGP/OSPF in the network simulator.
//!
//! References:
//! - draft-dekater-scion-controlplane-12
//! - scion-docs/SCION_BEACONING_REFERENCE.md
//! - scion-docs/SCION_IMPLEMENTATION_PLAN.md

// TODO: Complete documentation for all public items before finalizing
#![allow(missing_docs)]

/// Core SCION types (ISD, AS, interfaces, links)
pub mod types;
/// Path Construction Beacon (PCB) structures
pub mod pcb;
/// Beacon store for temporary PCB storage
pub mod beacon_store;
/// Path database for registered path segments
pub mod path_db;
/// SCION control plane events
pub mod event;

// Re-export commonly used types
pub use types::*;
pub use pcb::*;
pub use beacon_store::*;
pub use path_db::*;
pub use event::*;
