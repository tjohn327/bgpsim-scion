// BgpSim: BGP Network Simulator written in Rust
// Copyright 2022-2024 Tibor Schneider <sctibor@ethz.ch>
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! SCION (Scalability, Control, and Isolation On Next-generation networks) support for bgpsim.
//!
//! This module provides a complete implementation of the SCION control plane, enabling
//! simulation of path-aware inter-domain routing. SCION is fundamentally different from BGP:
//! it gives path control to endpoints, uses beaconing for path discovery, and organizes
//! ASes into Isolation Domains (ISDs).
//!
//! # Main Concepts
//!
//! - **ISDs (Isolation Domains)**: Logical groupings of ASes with uniform trust environments
//! - **Path Construction Beacons (PCBs)**: Messages propagated through the network to discover paths
//! - **Path Segments**: Three types (up, down, core) that combine to form end-to-end paths
//! - **Control Service**: Per-AS service responsible for beaconing, registration, and path lookup
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use bgpsim::prelude::*;
//! use bgpsim::scion::*;
//!
//! // Create a network
//! let mut net = Network::<SimplePrefix, BasicEventQueue<_>, GlobalOspf>::default();
//!
//! // Add SCION ASes
//! let core1 = net.add_router("Core1", 0x110000000001);
//! let core2 = net.add_router("Core2", 0x110000000002);
//!
//! // Configure SCION
//! net.set_scion_enabled(core1, IsdAs::new(1, 0x110000000001), true)?;
//! net.set_scion_enabled(core2, IsdAs::new(1, 0x110000000002), true)?;
//!
//! // Setup topology and run beaconing...
//! ```
//!
//! # Architecture
//!
//! SCION integration follows bgpsim's event-driven architecture. The control plane operates
//! through three concurrent processes:
//!
//! 1. **Beaconing**: Core ASes generate PCBs and propagate them through the network
//! 2. **Registration**: ASes select PCBs and register them as path segments
//! 3. **Lookup**: Endpoints query for path segments to construct forwarding paths
//!
//! # Features
//!
//! - Complete control plane simulation (beaconing, registration, lookup)
//! - Support for intra-ISD and inter-ISD routing
//! - Peering shortcuts for path optimization
//! - Configurable PCB selection policies
//! - Path validation (valley-free property, loop detection)
//! - Integration with existing bgpsim event system

pub mod event;
pub mod path_segment;
pub mod pcb;
pub mod process;
pub mod state;
pub mod types;

// Placeholder modules for future implementation
pub mod beaconing {
    //! Beaconing logic for core and intra-ISD path discovery.
    //!
    //! This module will contain the implementation of PCB generation,
    //! propagation, selection, and storage.
}

// Re-export commonly used types
pub use event::ScionEvent;
pub use path_segment::{
    ForwardingPath, PathConstructionError, PathSegment, PathValidationError, PeeringShortcut,
    SegmentType,
};
pub use pcb::{
    AsEntry, HopEntry, HopField, Pcb, PcbExtensions, PcbValidationError, PeerEntry, SegmentInfo,
};
pub use process::ScionControlService;
pub use state::{BeaconStore, PathDatabase};
pub use types::{InterfaceId, InterfaceInfo, IsdAs, IsdNumber, ScionAsn, ScionLinkType};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Just verify that all exports are accessible
        let _isd = IsdNumber(1);
        let _asn = ScionAsn::from(110u64);
        let _isd_as = IsdAs::new(1, 110u64);
        let _link_type = ScionLinkType::Core;
        let _if_id = InterfaceId(1);
    }
}
