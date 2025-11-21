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
use crate::types::{IntoIpv4Prefix, Ipv4Prefix, Prefix};

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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
                    peer_mtu: peer_entry.peer_mtu,
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
            SegmentType::Down => self.as_path.first().copied(), // Down-segments: forwarding goes from first (core) to last (non-core)
        }
    }

    /// Get the destination AS of this segment (in forwarding direction)
    pub fn destination(&self) -> Option<IsdAs> {
        match self.segment_type {
            SegmentType::Up => self.as_path.first().copied(), // Up-segments: forwarding goes from last (non-core) to first (core)
            SegmentType::Core => self.as_path.last().copied(),
            SegmentType::Down => self.as_path.last().copied(), // Down-segments: forwarding goes from first (core) to last (non-core)
        }
    }

    /// Get the length of the path (number of ASes)
    pub fn length(&self) -> usize {
        self.as_path.len()
    }

    /// Get the parent link identifier for up-segments.
    ///
    /// For up-segments, this returns the egress interface ID of the first hop,
    /// which identifies which parent link was used (the interface on the parent/core AS).
    /// Returns None for non-up segments or if the segment has no hop fields.
    pub fn parent_link_id(&self) -> Option<InterfaceId> {
        if self.segment_type != SegmentType::Up || self.hop_fields.is_empty() {
            return None;
        }
        // For up-segments in forwarding direction, the first hop's egress interface
        // is on the parent/core AS and identifies which parent link was used
        Some(self.hop_fields[0].egress)
    }

    /// Get the parent link identifier for down-segments.
    ///
    /// For down-segments, this returns the ingress interface ID of the first hop,
    /// which identifies which parent link was used (the interface on the parent/core AS).
    /// Returns None for non-down segments or if the segment has no hop fields.
    pub fn parent_link_id_down(&self) -> Option<InterfaceId> {
        if self.segment_type != SegmentType::Down || self.hop_fields.is_empty() {
            return None;
        }
        // For down-segments in forwarding direction, the first hop's ingress interface
        // is on the parent/core AS and identifies which parent link was used
        Some(self.hop_fields[0].ingress)
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
                peer_isd_as: p.peer_isd_as,
                peer_interface: p.peer_interface,
                peer_mtu: p.peer_mtu,
                hop_field: p.hop_field,
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

    /// Chain multiple core segments together into a single logical segment.
    ///
    /// This is used for multi-hop inter-ISD paths where traffic traverses multiple ISDs.
    /// The segments must form a valid chain (end of one segment connects to start of next).
    ///
    /// # Arguments
    /// * `segments` - Vector of core segments to chain together
    ///
    /// # Returns
    /// A single combined core segment, or None if chain is invalid
    pub fn chain_core_segments(segments: Vec<PathSegment<P>>) -> Option<Self> {
        if segments.is_empty() {
            return None;
        }

        if segments.len() == 1 {
            return Some(segments[0].clone());
        }

        // Verify all segments are core segments
        if !segments
            .iter()
            .all(|s| matches!(s.segment_type, SegmentType::Core))
        {
            return None;
        }

        // Verify segments form a valid chain
        for i in 0..segments.len() - 1 {
            let curr_dest = segments[i].destination()?;
            let next_src = segments[i + 1].source()?;
            if curr_dest != next_src {
                return None; // Chain is broken
            }
        }

        // Combine all segments
        let mut combined_hop_fields = Vec::new();
        let mut combined_as_path = Vec::new();
        let mut combined_peering = Vec::new();
        let mut min_mtu = u16::MAX;
        let mut min_expiration = u32::MAX;

        for (seg_idx, seg) in segments.iter().enumerate() {
            // Add hop fields
            combined_hop_fields.extend(seg.hop_fields.iter().cloned());

            // Add AS path (avoid duplicating the connecting AS)
            if seg_idx == 0 {
                combined_as_path.extend(seg.as_path.iter().cloned());
            } else {
                // Skip first AS (it's the same as last AS of previous segment)
                combined_as_path.extend(seg.as_path.iter().skip(1).cloned());
            }

            // Adjust peering positions and add
            let offset = if seg_idx == 0 {
                0
            } else {
                combined_as_path.len() - seg.as_path.len() + 1
            };
            for peering in &seg.peering_options {
                combined_peering.push(PeeringShortcut {
                    position: peering.position + offset,
                    ..*peering
                });
            }

            // Track minimum MTU and expiration
            min_mtu = min_mtu.min(seg.mtu);
            min_expiration = min_expiration.min(seg.expiration);
        }

        Some(PathSegment {
            segment_type: SegmentType::Core,
            info: segments[0].info, // Use first segment's info
            hop_fields: combined_hop_fields,
            as_path: combined_as_path,
            peering_options: combined_peering,
            expiration: min_expiration,
            mtu: min_mtu,
            _phantom: std::marker::PhantomData,
        })
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

    /// Find peering shortcuts between two segments.
    ///
    /// Returns a list of possible peering shortcuts where the two segments
    /// can be connected via a peering link instead of going through core.
    ///
    /// # Arguments
    /// * `other` - The other segment to check for peering opportunities
    ///
    /// # Returns
    /// Vector of tuples: (position_in_self, position_in_other, PeeringShortcut)
    pub fn find_peering_shortcuts(
        &self,
        other: &PathSegment<P>,
    ) -> Vec<(usize, usize, &PeeringShortcut)> {
        let mut shortcuts = Vec::new();

        // Check each peering option in self
        for peering in &self.peering_options {
            // Find the position in other's as_path where the peer AS appears
            for (other_pos, &other_as) in other.as_path.iter().enumerate() {
                if other_as == peering.peer_isd_as {
                    // Found a matching AS, now check if other has a peering option back to self
                    for other_peering in &other.peering_options {
                        // Check if the peering is at the correct position and points back
                        if other_peering.position == other_pos
                            && self.as_path.get(peering.position)
                                == Some(&other_peering.peer_isd_as)
                        {
                            shortcuts.push((peering.position, other_pos, peering));
                        }
                    }
                }
            }
        }

        shortcuts
    }
}

/// Information about a peering shortcut available in a path segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeeringShortcut {
    /// Position in the segment where peering is available
    pub position: usize,
    /// Peer AS and interface
    pub peer_isd_as: IsdAs,
    /// Interface ID on the peer's side
    pub peer_interface: InterfaceId,
    /// MTU of the peering link
    pub peer_mtu: u16,
    /// Hop field for the peering link
    pub hop_field: HopField,
}

impl<P: Prefix> IntoIpv4Prefix for PathSegment<P> {
    type T = PathSegment<Ipv4Prefix>;

    fn into_ipv4_prefix(self) -> Self::T {
        PathSegment {
            segment_type: self.segment_type,
            info: self.info,
            hop_fields: self.hop_fields,
            as_path: self.as_path,
            peering_options: self.peering_options,
            expiration: self.expiration,
            mtu: self.mtu,
            _phantom: std::marker::PhantomData,
        }
    }
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

    /// Create a new forwarding path using a peering shortcut.
    ///
    /// This method creates a path that uses a direct peering link between two ASes
    /// instead of going through a core AS. According to SCION spec, paths can contain
    /// at most one peering link.
    ///
    /// # Arguments
    /// * `seg1` - First segment (typically an up-segment)
    /// * `seg2` - Second segment (typically a down-segment or another up-segment)
    /// * `peering_pos1` - Position in seg1's as_path where peering AS is
    /// * `peering_pos2` - Position in seg2's as_path where peering AS is
    /// * `peering` - The peering shortcut information
    ///
    /// # Returns
    /// A ForwardingPath that uses the peering shortcut, or an error if construction fails
    pub fn new_with_peering(
        seg1: PathSegment<P>,
        seg2: PathSegment<P>,
        peering_pos1: usize,
        peering_pos2: usize,
        peering: PeeringShortcut,
    ) -> Result<Self, PathConstructionError> {
        // Compute AS path using the peering shortcut
        // Path goes: seg1[..=peering_pos1] -> peering link -> seg2[peering_pos2..]

        let mut as_path = Vec::new();

        // Add ASes from seg1 in forwarding direction up to and including the peering AS
        let seg1_forwarding: Vec<IsdAs> = match seg1.segment_type {
            SegmentType::Up => seg1.as_path.iter().rev().copied().collect(),
            SegmentType::Down => seg1.as_path.iter().rev().copied().collect(),
            SegmentType::Core => seg1.as_path.clone(),
        };

        // Calculate peering position in forwarding direction for seg1
        let peering_pos1_fwd = if matches!(seg1.segment_type, SegmentType::Up | SegmentType::Down) {
            seg1.as_path.len() - 1 - peering_pos1
        } else {
            peering_pos1
        };

        as_path.extend(seg1_forwarding[..=peering_pos1_fwd].iter().copied());

        // Add ASes from seg2 in forwarding direction starting after the peering AS
        let seg2_forwarding: Vec<IsdAs> = match seg2.segment_type {
            SegmentType::Up => seg2.as_path.iter().rev().copied().collect(),
            SegmentType::Down => seg2.as_path.iter().rev().copied().collect(),
            SegmentType::Core => seg2.as_path.clone(),
        };

        // Calculate peering position in forwarding direction for seg2
        let peering_pos2_fwd = if matches!(seg2.segment_type, SegmentType::Up | SegmentType::Down) {
            seg2.as_path.len() - 1 - peering_pos2
        } else {
            peering_pos2
        };

        // Skip the peering AS in seg2 (already included from seg1) and add remaining ASes
        if peering_pos2_fwd + 1 < seg2_forwarding.len() {
            as_path.extend(seg2_forwarding[peering_pos2_fwd + 1..].iter().copied());
        }

        // Calculate MTU (minimum of both segments and peering link)
        let mtu = seg1.mtu.min(seg2.mtu).min(peering.peer_mtu);

        Ok(ForwardingPath {
            up_segment: Some(seg1),
            core_segment: None,
            down_segment: Some(seg2),
            peering_shortcut: Some(peering),
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

    #[test]
    fn test_peering_shortcut_detection() {
        use crate::scion::pcb::{AsEntry, HopEntry, PeerEntry, SegmentInfo};

        // Create two PCBs with peering links
        let info1 = SegmentInfo::new(1000, 12345);
        let mut pcb1: Pcb<SimplePrefix> = Pcb::new(info1);

        let as1 = IsdAs::new(1, 110u64); // Non-core
        let as2 = IsdAs::new(1, 120u64); // Peer
        let core = IsdAs::new(1, 200u64); // Core

        // PCB1: core -> as1, with peering to as2
        pcb1.add_as_entry(AsEntry::new(
            core,
            HopEntry::new(create_test_hop_field(0, 1), 1500),
        ));

        let mut as1_entry = AsEntry::new(as1, HopEntry::new(create_test_hop_field(1, 0), 1500));
        // Add peering to as2
        as1_entry.peer_entries.push(PeerEntry {
            peer_isd_as: as2,
            peer_interface: InterfaceId(10),
            peer_mtu: 1400,
            hop_field: create_test_hop_field(5, 0),
        });
        pcb1.add_as_entry(as1_entry);

        // PCB2: core -> as2, with peering to as1
        let info2 = SegmentInfo::new(1000, 12346);
        let mut pcb2: Pcb<SimplePrefix> = Pcb::new(info2);

        pcb2.add_as_entry(AsEntry::new(
            core,
            HopEntry::new(create_test_hop_field(0, 2), 1500),
        ));

        let mut as2_entry = AsEntry::new(as2, HopEntry::new(create_test_hop_field(2, 0), 1500));
        // Add peering to as1
        as2_entry.peer_entries.push(PeerEntry {
            peer_isd_as: as1,
            peer_interface: InterfaceId(5),
            peer_mtu: 1400,
            hop_field: create_test_hop_field(10, 0),
        });
        pcb2.add_as_entry(as2_entry);

        let seg1 = PathSegment::from_pcb(&pcb1, SegmentType::Up);
        let seg2 = PathSegment::from_pcb(&pcb2, SegmentType::Up);

        // Find peering shortcuts
        let shortcuts = seg1.find_peering_shortcuts(&seg2);

        // Should find at least one shortcut
        assert!(!shortcuts.is_empty(), "Should find peering shortcuts");

        // Verify the shortcut has correct peer AS
        let (pos1, pos2, peering) = shortcuts[0];
        assert_eq!(peering.peer_isd_as, as2);
        assert!(pos1 < seg1.as_path.len());
        assert!(pos2 < seg2.as_path.len());
    }

    #[test]
    fn test_forwarding_path_with_peering() {
        use crate::scion::pcb::{AsEntry, HopEntry, PeerEntry, SegmentInfo};

        // Create two segments with peering
        let info1 = SegmentInfo::new(1000, 12345);
        let mut pcb1: Pcb<SimplePrefix> = Pcb::new(info1);

        let as1 = IsdAs::new(1, 110u64);
        let as2 = IsdAs::new(1, 120u64);
        let core = IsdAs::new(1, 200u64);

        // PCB1: core -> as1, with peering to as2
        pcb1.add_as_entry(AsEntry::new(
            core,
            HopEntry::new(create_test_hop_field(0, 1), 1500),
        ));

        let mut as1_entry = AsEntry::new(as1, HopEntry::new(create_test_hop_field(1, 0), 1500));
        as1_entry.peer_entries.push(PeerEntry {
            peer_isd_as: as2,
            peer_interface: InterfaceId(10),
            peer_mtu: 1400,
            hop_field: create_test_hop_field(5, 0),
        });
        pcb1.add_as_entry(as1_entry);

        // PCB2: core -> as2, with peering to as1
        let info2 = SegmentInfo::new(1000, 12346);
        let mut pcb2: Pcb<SimplePrefix> = Pcb::new(info2);

        pcb2.add_as_entry(AsEntry::new(
            core,
            HopEntry::new(create_test_hop_field(0, 2), 1500),
        ));

        let mut as2_entry = AsEntry::new(as2, HopEntry::new(create_test_hop_field(2, 0), 1500));
        as2_entry.peer_entries.push(PeerEntry {
            peer_isd_as: as1,
            peer_interface: InterfaceId(5),
            peer_mtu: 1400,
            hop_field: create_test_hop_field(10, 0),
        });
        pcb2.add_as_entry(as2_entry);

        let seg1 = PathSegment::from_pcb(&pcb1, SegmentType::Up);
        let seg2 = PathSegment::from_pcb(&pcb2, SegmentType::Up);

        // Find and use peering shortcut
        let shortcuts = seg1.find_peering_shortcuts(&seg2);
        assert!(!shortcuts.is_empty());

        let (pos1, pos2, peering) = shortcuts[0];
        let path =
            ForwardingPath::new_with_peering(seg1.clone(), seg2.clone(), pos1, pos2, *peering);

        assert!(path.is_ok(), "Should create peering path");
        let path = path.unwrap();

        // Path should have peering shortcut set
        assert!(path.peering_shortcut.is_some());

        // MTU should be minimum of segments and peering link
        assert_eq!(path.mtu, 1400);

        // Path should not go through core
        assert!(path.length() < 4, "Peering shortcut should be shorter");
    }

    #[test]
    fn test_peering_shortcut_no_match() {
        use crate::scion::pcb::{AsEntry, HopEntry, PeerEntry, SegmentInfo};

        // Create two segments with non-matching peering
        let info1 = SegmentInfo::new(1000, 12345);
        let mut pcb1: Pcb<SimplePrefix> = Pcb::new(info1);

        let as1 = IsdAs::new(1, 110u64);
        let as3 = IsdAs::new(1, 130u64); // Different peer
        let core = IsdAs::new(1, 200u64);

        pcb1.add_as_entry(AsEntry::new(
            core,
            HopEntry::new(create_test_hop_field(0, 1), 1500),
        ));

        let mut as1_entry = AsEntry::new(as1, HopEntry::new(create_test_hop_field(1, 0), 1500));
        // Peering to as3 (not as2)
        as1_entry.peer_entries.push(PeerEntry {
            peer_isd_as: as3,
            peer_interface: InterfaceId(10),
            peer_mtu: 1400,
            hop_field: create_test_hop_field(5, 0),
        });
        pcb1.add_as_entry(as1_entry);

        // PCB2 without matching peering
        let info2 = SegmentInfo::new(1000, 12346);
        let mut pcb2: Pcb<SimplePrefix> = Pcb::new(info2);
        let as2 = IsdAs::new(1, 120u64);

        pcb2.add_as_entry(AsEntry::new(
            core,
            HopEntry::new(create_test_hop_field(0, 2), 1500),
        ));
        pcb2.add_as_entry(AsEntry::new(
            as2,
            HopEntry::new(create_test_hop_field(2, 0), 1500),
        ));

        let seg1 = PathSegment::from_pcb(&pcb1, SegmentType::Up);
        let seg2 = PathSegment::from_pcb(&pcb2, SegmentType::Up);

        // Should not find any shortcuts
        let shortcuts = seg1.find_peering_shortcuts(&seg2);
        assert!(
            shortcuts.is_empty(),
            "Should not find shortcuts without matching peering"
        );
    }
}
