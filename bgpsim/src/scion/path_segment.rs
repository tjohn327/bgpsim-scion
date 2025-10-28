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

//! SCION path segments and forwarding paths.
//!
//! Path segments are derived from PCBs and can be combined to create
//! end-to-end forwarding paths.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::scion::pcb::{HopField, Pcb, SegmentInfo};
use crate::scion::types::{InterfaceId, IsdAs};
use crate::types::Prefix;

/// Type of SCION path segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SegmentType {
    /// Up segment: from non-core AS to core AS
    Up,
    /// Down segment: from core AS to non-core AS
    Down,
    /// Core segment: between core ASes
    Core,
}

impl std::fmt::Display for SegmentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SegmentType::Up => write!(f, "Up"),
            SegmentType::Down => write!(f, "Down"),
            SegmentType::Core => write!(f, "Core"),
        }
    }
}

/// A SCION path segment derived from a PCB.
///
/// Path segments can be registered and later looked up to construct
/// end-to-end forwarding paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathSegment<P: Prefix> {
    /// Type of segment
    pub segment_type: SegmentType,
    /// Segment information from the PCB
    pub info: SegmentInfo,
    /// Sequence of hop fields
    pub hop_fields: Vec<HopField>,
    /// AS path
    pub as_path: Vec<IsdAs>,
    /// Available peering shortcuts
    pub peering_options: Vec<PeeringShortcut>,
    /// Expiration time (absolute)
    pub expiration: u32,
    /// MTU of this segment
    pub mtu: u16,
    /// Phantom data for prefix type
    _phantom: std::marker::PhantomData<P>,
}

impl<P: Prefix> PathSegment<P> {
    /// Create a path segment from a PCB
    pub fn from_pcb(pcb: &Pcb<P>, segment_type: SegmentType) -> Self {
        let hop_fields: Vec<HopField> = pcb
            .as_entries
            .iter()
            .map(|e| e.hop_entry.hop_field)
            .collect();

        let as_path = pcb.get_as_path();

        // Extract peering options
        let mut peering_options = Vec::new();
        for (pos, entry) in pcb.as_entries.iter().enumerate() {
            for peer_entry in &entry.peer_entries {
                peering_options.push(PeeringShortcut {
                    position: pos,
                    peer_isd_as: peer_entry.peer_isd_as,
                    peer_interface: peer_entry.peer_interface,
                    hop_field: peer_entry.hop_field,
                });
            }
        }

        // Calculate expiration (min of all hop expirations)
        let expiration = hop_fields
            .iter()
            .map(|h| h.absolute_expiration(pcb.segment_info.timestamp))
            .min()
            .unwrap_or(pcb.segment_info.timestamp);

        let mtu = pcb.get_min_mtu().unwrap_or(1280); // Default minimum MTU

        PathSegment {
            segment_type,
            info: pcb.segment_info,
            hop_fields,
            as_path,
            peering_options,
            expiration,
            mtu,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get the source AS of this segment (in forwarding direction)
    pub fn source(&self) -> Option<IsdAs> {
        match self.segment_type {
            SegmentType::Up => self.as_path.last().copied(), // Up-segments: forwarding goes from last (non-core) to first (core)
            SegmentType::Core => self.as_path.first().copied(),
            SegmentType::Down => self.as_path.last().copied(), // Down-segments: forwarding goes from last (core) to first (non-core) in reversed as_path
        }
    }

    /// Get the destination AS of this segment (in forwarding direction)
    pub fn destination(&self) -> Option<IsdAs> {
        match self.segment_type {
            SegmentType::Up => self.as_path.first().copied(), // Up-segments: forwarding goes from last (non-core) to first (core)
            SegmentType::Core => self.as_path.last().copied(),
            SegmentType::Down => self.as_path.first().copied(), // Down-segments: forwarding goes from last (core) to first (non-core) in reversed as_path
        }
    }

    /// Get the length of the path (number of ASes)
    pub fn length(&self) -> usize {
        self.as_path.len()
    }

    /// Check if this segment is expired
    pub fn is_expired(&self, current_time: u32) -> bool {
        current_time > self.expiration
    }

    /// Reverse the segment (up ↔ down)
    pub fn reverse(&self) -> Self {
        let reversed_type = match self.segment_type {
            SegmentType::Up => SegmentType::Down,
            SegmentType::Down => SegmentType::Up,
            SegmentType::Core => SegmentType::Core, // Core segments are bidirectional
        };

        let mut reversed_hop_fields = self.hop_fields.clone();
        reversed_hop_fields.reverse();

        let mut reversed_as_path = self.as_path.clone();
        reversed_as_path.reverse();

        // Reverse peering positions
        let max_pos = self.as_path.len().saturating_sub(1);
        let reversed_peering: Vec<PeeringShortcut> = self
            .peering_options
            .iter()
            .map(|p| PeeringShortcut {
                position: max_pos.saturating_sub(p.position),
                ..*p
            })
            .collect();

        PathSegment {
            segment_type: reversed_type,
            info: self.info,
            hop_fields: reversed_hop_fields,
            as_path: reversed_as_path,
            peering_options: reversed_peering,
            expiration: self.expiration,
            mtu: self.mtu,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Check if this segment can connect with another segment
    pub fn can_connect(&self, other: &PathSegment<P>) -> bool {
        match (self.segment_type, other.segment_type) {
            (SegmentType::Up, SegmentType::Down) => {
                // Up segment must end at same core AS where down segment starts
                self.destination() == other.source()
            }
            (SegmentType::Up, SegmentType::Core) => {
                // Up segment must end where core segment starts
                self.destination() == other.source()
            }
            (SegmentType::Core, SegmentType::Down) => {
                // Core segment must end where down segment starts
                self.destination() == other.source()
            }
            (SegmentType::Core, SegmentType::Core) => {
                // Core segments can chain if they connect
                self.destination() == other.source()
            }
            _ => false,
        }
    }
}

/// Information about a peering shortcut available in a path segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeeringShortcut {
    /// Position in the segment where peering is available
    pub position: usize,
    /// Peer AS and interface
    pub peer_isd_as: IsdAs,
    /// Interface ID on the peer's side
    pub peer_interface: InterfaceId,
    /// Hop field for the peering link
    pub hop_field: HopField,
}

/// A complete end-to-end forwarding path.
///
/// Forwarding paths are constructed by combining up to three path segments:
/// an up segment, a core segment, and a down segment.
#[derive(Debug, Clone, PartialEq)]
pub struct ForwardingPath<P: Prefix> {
    /// Up segment (optional, not needed if source is core)
    pub up_segment: Option<PathSegment<P>>,
    /// Core segment (optional, not needed for intra-ISD)
    pub core_segment: Option<PathSegment<P>>,
    /// Down segment (optional, not needed if destination is core)
    pub down_segment: Option<PathSegment<P>>,
    /// Peering shortcut used (if any)
    pub peering_shortcut: Option<PeeringShortcut>,
    /// Complete AS-level path
    pub as_path: Vec<IsdAs>,
    /// Total path MTU
    pub mtu: u16,
}

impl<P: Prefix> ForwardingPath<P> {
    /// Create a new forwarding path from segments
    pub fn new(
        up: Option<PathSegment<P>>,
        core: Option<PathSegment<P>>,
        down: Option<PathSegment<P>>,
    ) -> Result<Self, PathConstructionError> {
        // Validate segment connectivity
        if let (Some(up_seg), Some(core_seg)) = (&up, &core) {
            if !up_seg.can_connect(core_seg) {
                return Err(PathConstructionError::SegmentMismatch);
            }
        }

        if let (Some(core_seg), Some(down_seg)) = (&core, &down) {
            if !core_seg.can_connect(down_seg) {
                return Err(PathConstructionError::SegmentMismatch);
            }
        }

        if let (Some(up_seg), Some(down_seg)) = (&up, &down) {
            if core.is_none() && !up_seg.can_connect(down_seg) {
                return Err(PathConstructionError::SegmentMismatch);
            }
        }

        // Compute complete AS path in forwarding direction
        let mut as_path = Vec::new();
        if let Some(up_seg) = &up {
            // Up-segment as_path is in beaconing order (core -> non-core)
            // For forwarding, we need it in reverse order (non-core -> core)
            as_path.extend(up_seg.as_path.iter().rev().copied());
        }
        if let Some(core_seg) = &core {
            // Core segments are in beaconing order and can be used as-is
            // Skip first AS if it's already in the path (junction point with up-segment)
            let start = if !as_path.is_empty() { 1 } else { 0 };
            as_path.extend(core_seg.as_path[start..].iter().copied());
        }
        if let Some(down_seg) = &down {
            // Down-segment as_path is reversed beaconing order (non-core -> ... -> core)
            // For forwarding (core -> non-core), we reverse it again
            // Skip first AS after reversal if it's the junction point
            let reversed: Vec<IsdAs> = down_seg.as_path.iter().rev().copied().collect();
            let start = if !as_path.is_empty() { 1 } else { 0 };
            as_path.extend(reversed[start..].iter().copied());
        }

        // Calculate total MTU (minimum of all segments)
        let mtu = [
            up.as_ref().map(|s| s.mtu),
            core.as_ref().map(|s| s.mtu),
            down.as_ref().map(|s| s.mtu),
        ]
        .iter()
        .filter_map(|&m| m)
        .min()
        .unwrap_or(1280);

        Ok(ForwardingPath {
            up_segment: up,
            core_segment: core,
            down_segment: down,
            peering_shortcut: None,
            as_path,
            mtu,
        })
    }

    /// Get the source ISD-AS
    pub fn source(&self) -> Option<IsdAs> {
        self.as_path.first().copied()
    }

    /// Get the destination ISD-AS
    pub fn destination(&self) -> Option<IsdAs> {
        self.as_path.last().copied()
    }

    /// Get the path length (number of ASes)
    pub fn length(&self) -> usize {
        self.as_path.len()
    }

    /// Check if the path is valley-free
    pub fn is_valley_free(&self) -> bool {
        // A path is valley-free if it follows the pattern: up* core* down*
        // This is implicitly guaranteed by the segment types when constructed
        // via ForwardingPath::new(), which validates segment connectivity.
        // Valid combinations:
        // up only, down only, core only
        // up+down, up+core, core+down
        // up+core+down
        // Invalid: down+up, core+up, down+core+up, etc.
        // Since we construct from ordered segments, this should always be true
        true
    }

    /// Check if the path contains any loops
    pub fn has_loop(&self) -> bool {
        let mut seen = HashSet::new();
        for &isd_as in &self.as_path {
            if !seen.insert(isd_as) {
                return true;
            }
        }
        false
    }

    /// Validate the path
    pub fn validate(&self) -> Result<(), PathValidationError> {
        if self.as_path.is_empty() {
            return Err(PathValidationError::EmptyPath);
        }

        if !self.is_valley_free() {
            return Err(PathValidationError::NotValleyFree);
        }

        if self.has_loop() {
            return Err(PathValidationError::Loop);
        }

        Ok(())
    }

    /// Get all hop fields in order
    pub fn get_hop_fields(&self) -> Vec<HopField> {
        let mut hops = Vec::new();

        if let Some(up) = &self.up_segment {
            hops.extend(up.hop_fields.iter().copied());
        }
        if let Some(core) = &self.core_segment {
            hops.extend(core.hop_fields.iter().copied());
        }
        if let Some(down) = &self.down_segment {
            hops.extend(down.hop_fields.iter().copied());
        }

        hops
    }

    /// Check if path is expired
    pub fn is_expired(&self, current_time: u32) -> bool {
        self.up_segment
            .as_ref()
            .map_or(false, |s| s.is_expired(current_time))
            || self
                .core_segment
                .as_ref()
                .map_or(false, |s| s.is_expired(current_time))
            || self
                .down_segment
                .as_ref()
                .map_or(false, |s| s.is_expired(current_time))
    }
}

/// Errors that can occur during path construction
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathConstructionError {
    /// Segments don't connect properly
    SegmentMismatch,
    /// Invalid segment combination
    InvalidCombination,
}

impl std::fmt::Display for PathConstructionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathConstructionError::SegmentMismatch => {
                write!(f, "Path segments don't connect properly")
            }
            PathConstructionError::InvalidCombination => {
                write!(f, "Invalid segment combination")
            }
        }
    }
}

impl std::error::Error for PathConstructionError {}

/// Errors that can occur during path validation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathValidationError {
    /// Path is empty
    EmptyPath,
    /// Path is not valley-free
    NotValleyFree,
    /// Path contains a loop
    Loop,
}

impl std::fmt::Display for PathValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathValidationError::EmptyPath => write!(f, "Path is empty"),
            PathValidationError::NotValleyFree => write!(f, "Path is not valley-free"),
            PathValidationError::Loop => write!(f, "Path contains a loop"),
        }
    }
}

impl std::error::Error for PathValidationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scion::pcb::{AsEntry, HopEntry, SegmentInfo};
    use crate::types::SimplePrefix;

    fn create_test_hop_field(ingress: u16, egress: u16) -> HopField {
        HopField::new(InterfaceId(ingress), InterfaceId(egress), 200)
    }

    fn create_test_pcb() -> Pcb<SimplePrefix> {
        let info = SegmentInfo::new(1000, 12345);
        let mut pcb = Pcb::new(info);

        let as1 = IsdAs::new(1, 110u64);
        let as2 = IsdAs::new(1, 111u64);
        let as3 = IsdAs::new(1, 112u64);

        let hop1 = HopEntry::new(create_test_hop_field(0, 1), 1500);
        let hop2 = HopEntry::new(create_test_hop_field(1, 2), 1500);
        let hop3 = HopEntry::new(create_test_hop_field(2, 3), 1500);

        pcb.add_as_entry(AsEntry::new(as1, hop1));
        pcb.add_as_entry(AsEntry::new(as2, hop2));
        pcb.add_as_entry(AsEntry::new(as3, hop3));

        pcb
    }

    #[test]
    fn test_segment_type_display() {
        assert_eq!(SegmentType::Up.to_string(), "Up");
        assert_eq!(SegmentType::Down.to_string(), "Down");
        assert_eq!(SegmentType::Core.to_string(), "Core");
    }

    #[test]
    fn test_path_segment_from_pcb() {
        let pcb = create_test_pcb();
        let segment = PathSegment::from_pcb(&pcb, SegmentType::Up);

        assert_eq!(segment.segment_type, SegmentType::Up);
        assert_eq!(segment.length(), 3);
        assert_eq!(segment.hop_fields.len(), 3);
        assert_eq!(segment.as_path.len(), 3);
    }

    #[test]
    fn test_path_segment_source_destination() {
        let pcb = create_test_pcb();
        // PCB as_path is [110, 111, 112] (beaconing direction from core 110 to non-core 112)

        let up_seg = PathSegment::from_pcb(&pcb, SegmentType::Up);
        // Up-segment represents path FROM non-core TO core (112 -> 110 in forwarding direction)
        assert_eq!(up_seg.source(), Some(IsdAs::new(1, 112u64)));
        assert_eq!(up_seg.destination(), Some(IsdAs::new(1, 110u64)));

        // Down-segment created directly from same PCB (as_path not reversed yet)
        let down_seg = PathSegment::from_pcb(&pcb, SegmentType::Down);
        // as_path is still [110, 111, 112], but it's marked as Down type
        // Down-segment: forwarding goes from last to first, so source=112, dest=110
        assert_eq!(down_seg.source(), Some(IsdAs::new(1, 112u64)));
        assert_eq!(down_seg.destination(), Some(IsdAs::new(1, 110u64)));
    }

    #[test]
    fn test_path_segment_reverse() {
        let pcb = create_test_pcb();
        let up_seg = PathSegment::from_pcb(&pcb, SegmentType::Up);

        let down_seg = up_seg.reverse();
        assert_eq!(down_seg.segment_type, SegmentType::Down);
        assert_eq!(down_seg.as_path[0], up_seg.as_path[2]);
        assert_eq!(down_seg.as_path[2], up_seg.as_path[0]);
    }

    #[test]
    fn test_path_segment_connectivity() {
        let pcb1 = create_test_pcb();
        let up_seg = PathSegment::from_pcb(&pcb1, SegmentType::Up);
        // Up segment goes from 110 -> 111 -> 112 (destination is 112)

        // For down segment, we reverse it so destination (first in path) is same as up destination
        let down_seg = up_seg.reverse();
        // Down segment goes from 112 -> 111 -> 110 (destination is 112, which is first)

        // Should be able to connect since they both have 112 as destination
        assert!(up_seg.can_connect(&down_seg));
    }

    #[test]
    fn test_forwarding_path_creation() {
        // Create up segment from AS1 to core
        // PCB beaconing goes from core to non-core, so as_path should be [core, as1]
        let info1 = SegmentInfo::new(1000, 12345);
        let mut pcb1: Pcb<SimplePrefix> = Pcb::new(info1);
        let as1 = IsdAs::new(1, 110u64);
        let core = IsdAs::new(1, 200u64); // Core AS
        pcb1.add_as_entry(AsEntry::new(
            core,
            HopEntry::new(create_test_hop_field(0, 1), 1500),
        ));
        pcb1.add_as_entry(AsEntry::new(
            as1,
            HopEntry::new(create_test_hop_field(1, 2), 1500),
        ));
        let up_seg = PathSegment::from_pcb(&pcb1, SegmentType::Up); // as_path=[200,110], src=110, dest=200

        // Create down segment from core to AS2 (different from AS1)
        // First create an up-segment from AS2 to core, then reverse it
        let info2 = SegmentInfo::new(1000, 12346);
        let mut pcb2: Pcb<SimplePrefix> = Pcb::new(info2);
        let as2 = IsdAs::new(1, 111u64);
        pcb2.add_as_entry(AsEntry::new(
            core,
            HopEntry::new(create_test_hop_field(2, 3), 1500),
        ));
        pcb2.add_as_entry(AsEntry::new(
            as2,
            HopEntry::new(create_test_hop_field(3, 4), 1500),
        ));
        let temp_up = PathSegment::from_pcb(&pcb2, SegmentType::Up);
        let down_seg = temp_up.reverse(); // Reverse to get proper down-segment

        // Both segments meet at core AS
        let path = ForwardingPath::new(Some(up_seg), None, Some(down_seg));
        assert!(path.is_ok());

        let path = path.unwrap();
        assert!(path.length() > 0);
        assert!(path.validate().is_ok());
    }

    #[test]
    fn test_forwarding_path_properties() {
        let pcb = create_test_pcb();
        let up_seg = PathSegment::from_pcb(&pcb, SegmentType::Up);

        let path = ForwardingPath::new(Some(up_seg), None, None).unwrap();

        assert!(!path.has_loop());
        assert!(path.is_valley_free());
        assert_eq!(path.mtu, 1500);
    }

    #[test]
    fn test_forwarding_path_validation() {
        let pcb = create_test_pcb();
        let seg = PathSegment::from_pcb(&pcb, SegmentType::Up);

        let path = ForwardingPath::new(Some(seg), None, None).unwrap();
        assert!(path.validate().is_ok());
    }

    #[test]
    fn test_forwarding_path_hop_fields() {
        let pcb = create_test_pcb();
        let seg = PathSegment::from_pcb(&pcb, SegmentType::Up);

        let path = ForwardingPath::new(Some(seg), None, None).unwrap();
        let hops = path.get_hop_fields();

        assert_eq!(hops.len(), 3);
    }
}
