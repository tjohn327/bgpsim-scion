// Path Construction Beacon (PCB) structures
//
// PCBs are the fundamental building blocks of SCION's path discovery.
// They accumulate AS entries as they traverse the network during beaconing.

use super::types::*;
use std::time::{SystemTime, UNIX_EPOCH};

/// Path Construction Beacon
///
/// A PCB accumulates AS entries as it traverses the network. Each AS that
/// receives a PCB may extend it by appending its own AS entry and forwarding
/// it to neighbors.
#[derive(Debug, Clone)]
pub struct Pcb {
    pub segment_info: SegmentInfo,
    pub as_entries: Vec<AsEntry>,
}

impl Pcb {
    /// Create a new PCB (typically by a core AS)
    pub fn new(_origin: IsdAs) -> Self {
        Self {
            segment_info: SegmentInfo::new(),
            as_entries: Vec::new(),
        }
    }

    /// Create a PCB with specific segment info
    pub fn with_segment_info(segment_info: SegmentInfo) -> Self {
        Self {
            segment_info,
            as_entries: Vec::new(),
        }
    }

    /// Get the source ISD-AS (first entry)
    pub fn src(&self) -> Option<IsdAs> {
        self.as_entries.first().map(|e| e.isd_as)
    }

    /// Get the current destination ISD-AS (last entry)
    pub fn dst(&self) -> Option<IsdAs> {
        self.as_entries.last().map(|e| e.isd_as)
    }

    /// Get the length of the path (number of AS entries)
    pub fn len(&self) -> usize {
        self.as_entries.len()
    }

    /// Check if the PCB is empty (no AS entries)
    pub fn is_empty(&self) -> bool {
        self.as_entries.is_empty()
    }

    /// Extend PCB with a new AS entry
    pub fn extend(&mut self, entry: AsEntry) {
        self.as_entries.push(entry);
    }

    /// Check if this PCB is terminated (last entry has no next_isd_as)
    pub fn is_terminated(&self) -> bool {
        self.as_entries
            .last()
            .map(|e| e.next_isd_as.is_none())
            .unwrap_or(false)
    }

    /// Check if this PCB contains a specific ISD-AS
    pub fn contains(&self, isd_as: IsdAs) -> bool {
        self.as_entries.iter().any(|e| e.isd_as == isd_as)
    }

    /// Get all ISD-AS numbers in the path
    pub fn as_path(&self) -> Vec<IsdAs> {
        self.as_entries.iter().map(|e| e.isd_as).collect()
    }
}

/// Segment information (timestamp and ID)
///
/// This information is set by the originating AS and remains constant
/// as the PCB is propagated.
#[derive(Debug, Clone, Copy)]
pub struct SegmentInfo {
    /// Creation time (seconds since UNIX epoch)
    pub timestamp: i64,

    /// 16-bit cryptographically random identifier
    pub segment_id: u16,
}

impl SegmentInfo {
    /// Create new segment info with current timestamp and derived ID
    ///
    /// For simulation purposes, segment_id is derived from the timestamp
    /// rather than being cryptographically random.
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Self {
            timestamp,
            // Use lower 16 bits of timestamp as segment ID
            // (sufficient uniqueness for simulation)
            segment_id: (timestamp & 0xFFFF) as u16,
        }
    }

    /// Create segment info with specific values (for testing)
    pub fn with_values(timestamp: i64, segment_id: u16) -> Self {
        Self {
            timestamp,
            segment_id,
        }
    }
}

impl Default for SegmentInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// AS entry in a PCB
///
/// Each AS entry contains the information needed to forward packets through
/// that AS in the beaconing direction.
#[derive(Debug, Clone)]
pub struct AsEntry {
    /// ISD-AS number of this AS
    pub isd_as: IsdAs,

    /// Next ISD-AS in the path (None for terminated segments)
    pub next_isd_as: Option<IsdAs>,

    /// Hop field for data plane forwarding
    pub hop_entry: HopEntry,

    /// Optional peering link advertisements
    pub peer_entries: Vec<PeerEntry>,
}

impl AsEntry {
    /// Create a new AS entry
    pub fn new(
        isd_as: IsdAs,
        next_isd_as: Option<IsdAs>,
        hop_entry: HopEntry,
    ) -> Self {
        Self {
            isd_as,
            next_isd_as,
            hop_entry,
            peer_entries: Vec::new(),
        }
    }

    /// Add a peer entry to this AS entry
    pub fn add_peer_entry(&mut self, peer: PeerEntry) {
        self.peer_entries.push(peer);
    }

    /// Check if this is a terminating entry (no next AS)
    pub fn is_terminal(&self) -> bool {
        self.next_isd_as.is_none()
    }
}

/// Hop field information
///
/// Specifies ingress and egress interfaces for packet forwarding through an AS.
/// In the real SCION protocol, hop fields also contain a MAC for validation.
#[derive(Debug, Clone, Copy)]
pub struct HopEntry {
    /// Ingress interface ID
    pub ingress: InterfaceId,

    /// Egress interface ID (None for terminated segments)
    pub egress: Option<InterfaceId>,

    /// Encoded expiration time
    /// Real value: (1 + exp_time) * (86400s / 256)
    pub exp_time: u8,
}

impl HopEntry {
    /// Create a new hop entry
    pub fn new(ingress: InterfaceId, egress: Option<InterfaceId>) -> Self {
        Self {
            ingress,
            egress,
            exp_time: Self::default_exp_time(),
        }
    }

    /// Create a hop entry with specific expiration time
    pub fn with_exp_time(ingress: InterfaceId, egress: Option<InterfaceId>, exp_time: u8) -> Self {
        Self {
            ingress,
            egress,
            exp_time,
        }
    }

    /// Default expiration time encoding for ~6 hours
    /// exp_time = (6 * 3600 * 256 / 86400) - 1 ≈ 64
    fn default_exp_time() -> u8 {
        64
    }

    /// Calculate actual expiration duration in seconds
    pub fn expiration_seconds(&self) -> u64 {
        ((self.exp_time as u64 + 1) * 86400) / 256
    }

    /// Check if this hop is terminal (no egress)
    pub fn is_terminal(&self) -> bool {
        self.egress.is_none()
    }
}

/// Peer entry (advertised peering link)
///
/// Peering links are not traversed by PCBs directly, but are advertised
/// in AS entries so that segment combination can use them as shortcuts.
#[derive(Debug, Clone)]
pub struct PeerEntry {
    /// ISD-AS of the peer
    pub peer_isd_as: IsdAs,

    /// Interface ID on the peer's side
    pub peer_interface: InterfaceId,

    /// MTU of the peering link
    pub peer_mtu: u16,

    /// Hop field for the peering link
    pub hop_field: HopEntry,
}

impl PeerEntry {
    /// Create a new peer entry
    pub fn new(
        peer_isd_as: IsdAs,
        peer_interface: InterfaceId,
        hop_field: HopEntry,
    ) -> Self {
        Self {
            peer_isd_as,
            peer_interface,
            peer_mtu: 1500,
            hop_field,
        }
    }

    /// Create a peer entry with custom MTU
    pub fn with_mtu(
        peer_isd_as: IsdAs,
        peer_interface: InterfaceId,
        hop_field: HopEntry,
        mtu: u16,
    ) -> Self {
        Self {
            peer_isd_as,
            peer_interface,
            peer_mtu: mtu,
            hop_field,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_info_creation() {
        let info = SegmentInfo::new();
        assert!(info.timestamp > 0);
        // segment_id is random, just check it exists
        let _ = info.segment_id;
    }

    #[test]
    fn test_segment_info_with_values() {
        let info = SegmentInfo::with_values(1234567890, 42);
        assert_eq!(info.timestamp, 1234567890);
        assert_eq!(info.segment_id, 42);
    }

    #[test]
    fn test_hop_entry() {
        let hop = HopEntry::new(InterfaceId::new(1), Some(InterfaceId::new(2)));
        assert_eq!(hop.ingress.as_u16(), 1);
        assert_eq!(hop.egress.unwrap().as_u16(), 2);
        assert!(!hop.is_terminal());

        let terminal_hop = HopEntry::new(InterfaceId::new(1), None);
        assert!(terminal_hop.is_terminal());
    }

    #[test]
    fn test_hop_expiration() {
        let hop = HopEntry::with_exp_time(
            InterfaceId::new(1),
            Some(InterfaceId::new(2)),
            64,
        );
        let exp_seconds = hop.expiration_seconds();
        // (64 + 1) * 86400 / 256 = 21937.5 seconds ≈ 6.1 hours
        assert!(exp_seconds > 21000 && exp_seconds < 22000);
    }

    #[test]
    fn test_as_entry() {
        let isd_as = IsdAs::new(1, 100);
        let next = IsdAs::new(1, 101);
        let hop = HopEntry::new(InterfaceId::new(0), Some(InterfaceId::new(1)));

        let entry = AsEntry::new(isd_as, Some(next), hop);
        assert_eq!(entry.isd_as, isd_as);
        assert_eq!(entry.next_isd_as, Some(next));
        assert!(!entry.is_terminal());

        let terminal_entry = AsEntry::new(isd_as, None, hop);
        assert!(terminal_entry.is_terminal());
    }

    #[test]
    fn test_peer_entry() {
        let peer = PeerEntry::new(
            IsdAs::new(1, 200),
            InterfaceId::new(5),
            HopEntry::new(InterfaceId::new(3), Some(InterfaceId::new(4))),
        );

        assert_eq!(peer.peer_isd_as, IsdAs::new(1, 200));
        assert_eq!(peer.peer_interface.as_u16(), 5);
        assert_eq!(peer.peer_mtu, 1500);
    }

    #[test]
    fn test_pcb_creation() {
        let origin = IsdAs::new(1, 100);
        let pcb = Pcb::new(origin);

        assert_eq!(pcb.len(), 0);
        assert!(pcb.is_empty());
        assert_eq!(pcb.src(), None);
        assert_eq!(pcb.dst(), None);
        assert!(!pcb.is_terminated());
    }

    #[test]
    fn test_pcb_extension() {
        let origin = IsdAs::new(1, 100);
        let mut pcb = Pcb::new(origin);

        let entry1 = AsEntry::new(
            origin,
            Some(IsdAs::new(1, 101)),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        );
        pcb.extend(entry1);

        assert_eq!(pcb.len(), 1);
        assert_eq!(pcb.src(), Some(origin));
        assert_eq!(pcb.dst(), Some(origin));

        let next = IsdAs::new(1, 101);
        let entry2 = AsEntry::new(
            next,
            None,  // Terminal
            HopEntry::new(InterfaceId::new(1), None),
        );
        pcb.extend(entry2);

        assert_eq!(pcb.len(), 2);
        assert_eq!(pcb.src(), Some(origin));
        assert_eq!(pcb.dst(), Some(next));
        assert!(pcb.is_terminated());
    }

    #[test]
    fn test_pcb_contains() {
        let mut pcb = Pcb::new(IsdAs::new(1, 100));

        let as1 = IsdAs::new(1, 100);
        let as2 = IsdAs::new(1, 101);
        let as3 = IsdAs::new(1, 102);

        pcb.extend(AsEntry::new(
            as1,
            Some(as2),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));

        pcb.extend(AsEntry::new(
            as2,
            None,
            HopEntry::new(InterfaceId::new(1), None),
        ));

        assert!(pcb.contains(as1));
        assert!(pcb.contains(as2));
        assert!(!pcb.contains(as3));
    }

    #[test]
    fn test_pcb_as_path() {
        let mut pcb = Pcb::new(IsdAs::new(1, 100));

        let as1 = IsdAs::new(1, 100);
        let as2 = IsdAs::new(1, 101);
        let as3 = IsdAs::new(1, 102);

        pcb.extend(AsEntry::new(
            as1,
            Some(as2),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));

        pcb.extend(AsEntry::new(
            as2,
            Some(as3),
            HopEntry::new(InterfaceId::new(1), Some(InterfaceId::new(2))),
        ));

        pcb.extend(AsEntry::new(
            as3,
            None,
            HopEntry::new(InterfaceId::new(2), None),
        ));

        let path = pcb.as_path();
        assert_eq!(path, vec![as1, as2, as3]);
    }
}
