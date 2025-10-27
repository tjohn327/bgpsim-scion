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

//! Path Construction Beacon (PCB) structures and operations.
//!
//! PCBs are the core routing messages in SCION. They are initiated by core ASes
//! and propagated through the network, accumulating path information at each hop.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::scion::types::{InterfaceId, IsdAs};
use crate::types::Prefix;

/// Path Construction Beacon - the core routing message in SCION.
///
/// PCBs are initiated by core ASes and propagated through the network,
/// accumulating cryptographically protected path information at each AS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pcb<P: Prefix> {
    /// Segment information
    pub segment_info: SegmentInfo,
    /// AS entries accumulated during beaconing
    pub as_entries: Vec<AsEntry>,
    /// Optional extensions for additional metadata
    pub extensions: PcbExtensions,
    /// Phantom data for prefix type
    _phantom: std::marker::PhantomData<P>,
}

impl<P: Prefix> Pcb<P> {
    /// Create a new PCB (typically done by a core AS)
    pub fn new(segment_info: SegmentInfo) -> Self {
        Pcb {
            segment_info,
            as_entries: Vec::new(),
            extensions: PcbExtensions::default(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Add an AS entry to the PCB (extending the path)
    pub fn add_as_entry(&mut self, entry: AsEntry) {
        self.as_entries.push(entry);
    }

    /// Get the AS path represented by this PCB
    pub fn get_as_path(&self) -> Vec<IsdAs> {
        self.as_entries.iter().map(|e| e.isd_as).collect()
    }

    /// Get the length of the AS path
    pub fn path_length(&self) -> usize {
        self.as_entries.len()
    }

    /// Check if the PCB contains a loop (duplicate AS)
    pub fn has_loop(&self) -> bool {
        let mut seen = HashSet::new();
        for entry in &self.as_entries {
            if !seen.insert(entry.isd_as) {
                return true;
            }
        }
        false
    }

    /// Check if the PCB is expired
    pub fn is_expired(&self, current_time: u32, tolerance: u32) -> bool {
        // Check if timestamp is in the future (with tolerance)
        if self.segment_info.timestamp > current_time + tolerance {
            return true;
        }

        // Check if any hop is expired
        for entry in &self.as_entries {
            if entry.hop_entry.is_expired(self.segment_info.timestamp, current_time, tolerance) {
                return true;
            }
        }

        false
    }

    /// Validate the PCB
    pub fn validate(&self, current_time: u32, tolerance: u32) -> Result<(), PcbValidationError> {
        // Check timestamp validity
        if self.segment_info.timestamp > current_time + tolerance {
            return Err(PcbValidationError::FutureTimestamp);
        }

        // Check for loops
        if self.has_loop() {
            return Err(PcbValidationError::Loop);
        }

        // Check hop expiration
        if self.is_expired(current_time, tolerance) {
            return Err(PcbValidationError::Expired);
        }

        // Check continuity (consecutive AS entries should be valid)
        if self.as_entries.len() > 1 {
            for i in 0..self.as_entries.len() - 1 {
                // Verify that egress of current matches ingress of next
                // (simplified check - in real SCION this would be more complex)
                if self.as_entries[i].hop_entry.hop_field.egress == InterfaceId::UNSPECIFIED {
                    return Err(PcbValidationError::InvalidHopField);
                }
            }
        }

        Ok(())
    }

    /// Get the minimum MTU along the path
    pub fn get_min_mtu(&self) -> Option<u16> {
        self.as_entries
            .iter()
            .map(|e| e.hop_entry.ingress_mtu)
            .min()
    }

    /// Get the originating core AS (first AS in path)
    pub fn get_origin(&self) -> Option<IsdAs> {
        self.as_entries.first().map(|e| e.isd_as)
    }

    /// Get the last AS in the path
    pub fn get_last_as(&self) -> Option<IsdAs> {
        self.as_entries.last().map(|e| e.isd_as)
    }
}

/// Errors that can occur during PCB validation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PcbValidationError {
    /// PCB timestamp is in the future
    FutureTimestamp,
    /// PCB contains a loop (duplicate AS)
    Loop,
    /// PCB has expired
    Expired,
    /// Invalid hop field
    InvalidHopField,
    /// Continuity check failed
    ContinuityFailure,
}

impl std::fmt::Display for PcbValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PcbValidationError::FutureTimestamp => write!(f, "PCB timestamp is in the future"),
            PcbValidationError::Loop => write!(f, "PCB contains a loop"),
            PcbValidationError::Expired => write!(f, "PCB has expired"),
            PcbValidationError::InvalidHopField => write!(f, "Invalid hop field"),
            PcbValidationError::ContinuityFailure => write!(f, "Continuity check failed"),
        }
    }
}

impl std::error::Error for PcbValidationError {}

/// Segment information contained in a PCB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentInfo {
    /// Timestamp when the PCB was created (in seconds)
    pub timestamp: u32,
    /// Unique segment ID
    pub segment_id: u64,
    /// Flags for segment properties
    pub flags: SegmentFlags,
}

impl SegmentInfo {
    /// Create new segment info
    pub fn new(timestamp: u32, segment_id: u64) -> Self {
        SegmentInfo {
            timestamp,
            segment_id,
            flags: SegmentFlags::default(),
        }
    }
}

/// Flags for segment properties
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentFlags {
    /// Reserved for future use
    pub reserved: u16,
}

impl Default for SegmentFlags {
    fn default() -> Self {
        SegmentFlags { reserved: 0 }
    }
}

/// Hop field containing forwarding information.
///
/// Hop fields are used in the data plane for packet forwarding and
/// are authenticated with a MAC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopField {
    /// Ingress interface ID (in beaconing direction)
    pub ingress: InterfaceId,
    /// Egress interface ID (in beaconing direction)
    pub egress: InterfaceId,
    /// Encoded expiration time (8-bit)
    pub exp_time: u8,
    /// Message Authentication Code (simplified as u64)
    pub mac: u64,
}

impl HopField {
    /// Create a new hop field
    pub fn new(ingress: InterfaceId, egress: InterfaceId, exp_time: u8) -> Self {
        HopField {
            ingress,
            egress,
            exp_time,
            mac: 0, // MAC would be computed in real implementation
        }
    }

    /// Calculate the absolute expiration time in seconds
    ///
    /// Formula: absolute_expiration = segment_timestamp + duration
    /// where duration = (1 + exp_time) * (24*60*60/256)
    pub fn absolute_expiration(&self, segment_timestamp: u32) -> u32 {
        let duration = ((1 + self.exp_time as u32) * 24 * 60 * 60) / 256;
        segment_timestamp.saturating_add(duration)
    }

    /// Check if the hop field is expired
    pub fn is_expired(&self, segment_timestamp: u32, current_time: u32, tolerance: u32) -> bool {
        let expiration = self.absolute_expiration(segment_timestamp);
        current_time > expiration + tolerance
    }

    /// Compute a placeholder MAC (in real SCION, this would be cryptographic)
    pub fn compute_mac(&mut self, _key: &[u8]) {
        // Simplified MAC computation for simulation
        // In real SCION, this would be a proper HMAC or AES-CMAC
        self.mac = self.ingress.0 as u64 ^ (self.egress.0 as u64) << 16;
    }
}

/// Hop entry combining hop field with MTU information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HopEntry {
    /// The hop field
    pub hop_field: HopField,
    /// Ingress MTU (in beaconing direction)
    pub ingress_mtu: u16,
}

impl HopEntry {
    /// Create a new hop entry
    pub fn new(hop_field: HopField, ingress_mtu: u16) -> Self {
        HopEntry {
            hop_field,
            ingress_mtu,
        }
    }

    /// Check if this hop entry is expired
    pub fn is_expired(&self, segment_timestamp: u32, current_time: u32, tolerance: u32) -> bool {
        self.hop_field
            .is_expired(segment_timestamp, current_time, tolerance)
    }
}

/// AS entry in a PCB representing one hop in the path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsEntry {
    /// ISD-AS of this AS
    pub isd_as: IsdAs,
    /// Hop entry for forwarding through this AS
    pub hop_entry: HopEntry,
    /// Peering links from this AS (if any)
    pub peer_entries: Vec<PeerEntry>,
    /// MTU information (same as hop_entry.ingress_mtu, kept for API compatibility)
    pub ingress_mtu: u16,
    /// Signature over this AS entry (simplified as Vec<u8>)
    pub signature: Vec<u8>,
}

impl AsEntry {
    /// Create a new AS entry
    pub fn new(isd_as: IsdAs, hop_entry: HopEntry) -> Self {
        AsEntry {
            isd_as,
            ingress_mtu: hop_entry.ingress_mtu,
            hop_entry,
            peer_entries: Vec::new(),
            signature: Vec::new(),
        }
    }

    /// Add a peer entry
    pub fn add_peer_entry(&mut self, peer: PeerEntry) {
        self.peer_entries.push(peer);
    }

    /// Sign this AS entry (placeholder for cryptographic signature)
    pub fn sign(&mut self, _private_key: &[u8]) {
        // Simplified signature for simulation
        // In real SCION, this would be a proper digital signature
        let mut data = Vec::new();
        data.extend_from_slice(&self.isd_as.isd.0.to_be_bytes());
        data.extend_from_slice(&self.isd_as.asn.as_u64().to_be_bytes());
        self.signature = data;
    }

    /// Verify the signature (placeholder)
    pub fn verify_signature(&self, _public_key: &[u8]) -> bool {
        // Simplified verification for simulation
        !self.signature.is_empty()
    }
}

/// Peer entry describing a peering link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerEntry {
    /// ISD-AS of the peer
    pub peer_isd_as: IsdAs,
    /// Interface ID of the peering link on the peer's side
    pub peer_interface: InterfaceId,
    /// MTU of the peering link
    pub peer_mtu: u16,
    /// Hop field for the peering link
    pub hop_field: HopField,
}

impl PeerEntry {
    /// Create a new peer entry
    pub fn new(
        peer_isd_as: IsdAs,
        peer_interface: InterfaceId,
        peer_mtu: u16,
        hop_field: HopField,
    ) -> Self {
        PeerEntry {
            peer_isd_as,
            peer_interface,
            peer_mtu,
            hop_field,
        }
    }
}

/// PCB extensions for carrying additional metadata.
///
/// Initially empty, but can be extended with things like StaticInfoExtension
/// for latency, bandwidth, geolocation, etc.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PcbExtensions {
    /// Reserved for future extensions
    _reserved: (),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SimplePrefix;

    #[test]
    fn test_segment_info() {
        let info = SegmentInfo::new(1000, 12345);
        assert_eq!(info.timestamp, 1000);
        assert_eq!(info.segment_id, 12345);
    }

    #[test]
    fn test_hop_field() {
        let hop = HopField::new(InterfaceId(1), InterfaceId(2), 128);
        assert_eq!(hop.ingress.0, 1);
        assert_eq!(hop.egress.0, 2);
        assert_eq!(hop.exp_time, 128);

        // Test expiration calculation
        let segment_ts = 1000;
        let abs_exp = hop.absolute_expiration(segment_ts);
        assert!(abs_exp > segment_ts);

        // Not expired if current time is before expiration
        assert!(!hop.is_expired(segment_ts, segment_ts + 100, 10));

        // Expired if current time is after expiration + tolerance
        assert!(hop.is_expired(segment_ts, abs_exp + 100, 10));
    }

    #[test]
    fn test_hop_entry() {
        let hop_field = HopField::new(InterfaceId(1), InterfaceId(2), 128);
        let entry = HopEntry::new(hop_field, 1500);
        assert_eq!(entry.ingress_mtu, 1500);
    }

    #[test]
    fn test_as_entry() {
        let isd_as = IsdAs::new(1, 110u64);
        let hop_field = HopField::new(InterfaceId(1), InterfaceId(2), 128);
        let hop_entry = HopEntry::new(hop_field, 1500);
        let mut as_entry = AsEntry::new(isd_as, hop_entry);

        assert_eq!(as_entry.isd_as, isd_as);
        assert_eq!(as_entry.ingress_mtu, 1500);

        // Test peer entry
        let peer_hop = HopField::new(InterfaceId(3), InterfaceId(4), 128);
        let peer = PeerEntry::new(IsdAs::new(1, 111u64), InterfaceId(5), 1400, peer_hop);
        as_entry.add_peer_entry(peer);
        assert_eq!(as_entry.peer_entries.len(), 1);

        // Test signing
        as_entry.sign(&[]);
        assert!(!as_entry.signature.is_empty());
        assert!(as_entry.verify_signature(&[]));
    }

    #[test]
    fn test_pcb_creation() {
        let info = SegmentInfo::new(1000, 12345);
        let pcb: Pcb<SimplePrefix> = Pcb::new(info);

        assert_eq!(pcb.segment_info.timestamp, 1000);
        assert_eq!(pcb.as_entries.len(), 0);
    }

    #[test]
    fn test_pcb_add_entry() {
        let info = SegmentInfo::new(1000, 12345);
        let mut pcb: Pcb<SimplePrefix> = Pcb::new(info);

        let isd_as1 = IsdAs::new(1, 110u64);
        let hop_field1 = HopField::new(InterfaceId::UNSPECIFIED, InterfaceId(1), 200);
        let hop_entry1 = HopEntry::new(hop_field1, 1500);
        let as_entry1 = AsEntry::new(isd_as1, hop_entry1);

        pcb.add_as_entry(as_entry1);
        assert_eq!(pcb.path_length(), 1);

        let path = pcb.get_as_path();
        assert_eq!(path.len(), 1);
        assert_eq!(path[0], isd_as1);
    }

    #[test]
    fn test_pcb_loop_detection() {
        let info = SegmentInfo::new(1000, 12345);
        let mut pcb: Pcb<SimplePrefix> = Pcb::new(info);

        let isd_as = IsdAs::new(1, 110u64);
        let hop_field = HopField::new(InterfaceId::UNSPECIFIED, InterfaceId(1), 200);
        let hop_entry = HopEntry::new(hop_field, 1500);

        // Add same AS twice
        pcb.add_as_entry(AsEntry::new(isd_as, hop_entry));
        pcb.add_as_entry(AsEntry::new(isd_as, hop_entry));

        assert!(pcb.has_loop());
    }

    #[test]
    fn test_pcb_validation() {
        let info = SegmentInfo::new(1000, 12345);
        let mut pcb: Pcb<SimplePrefix> = Pcb::new(info);

        let isd_as = IsdAs::new(1, 110u64);
        let hop_field = HopField::new(InterfaceId::UNSPECIFIED, InterfaceId(1), 200);
        let hop_entry = HopEntry::new(hop_field, 1500);
        pcb.add_as_entry(AsEntry::new(isd_as, hop_entry));

        // Valid PCB
        assert!(pcb.validate(2000, 1000).is_ok());

        // Future timestamp
        assert!(matches!(
            pcb.validate(900, 10),
            Err(PcbValidationError::FutureTimestamp)
        ));
    }

    #[test]
    fn test_pcb_mtu() {
        let info = SegmentInfo::new(1000, 12345);
        let mut pcb: Pcb<SimplePrefix> = Pcb::new(info);

        let hop1 = HopEntry::new(HopField::new(InterfaceId(0), InterfaceId(1), 200), 1500);
        let hop2 = HopEntry::new(HopField::new(InterfaceId(1), InterfaceId(2), 200), 1400);
        let hop3 = HopEntry::new(HopField::new(InterfaceId(2), InterfaceId(3), 200), 1600);

        pcb.add_as_entry(AsEntry::new(IsdAs::new(1, 110u64), hop1));
        pcb.add_as_entry(AsEntry::new(IsdAs::new(1, 111u64), hop2));
        pcb.add_as_entry(AsEntry::new(IsdAs::new(1, 112u64), hop3));

        assert_eq!(pcb.get_min_mtu(), Some(1400));
    }

    #[test]
    fn test_pcb_origin_and_last() {
        let info = SegmentInfo::new(1000, 12345);
        let mut pcb: Pcb<SimplePrefix> = Pcb::new(info);

        let isd_as1 = IsdAs::new(1, 110u64);
        let isd_as2 = IsdAs::new(1, 111u64);

        let hop_field = HopField::new(InterfaceId::UNSPECIFIED, InterfaceId(1), 200);
        let hop_entry = HopEntry::new(hop_field, 1500);

        pcb.add_as_entry(AsEntry::new(isd_as1, hop_entry));
        pcb.add_as_entry(AsEntry::new(isd_as2, hop_entry));

        assert_eq!(pcb.get_origin(), Some(isd_as1));
        assert_eq!(pcb.get_last_as(), Some(isd_as2));
    }
}
