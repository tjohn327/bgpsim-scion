// SCION Control Service - AS-level coordinator for SCION operations
//
// Each ISD-AS has one ScionControlService shared by all routers in that AS.
// This service manages:
// - SCION interfaces (external links between ASes)
// - Border routers
// - Beacon store and path database
// - PCB propagation and path segment registration

use super::{types::*, BeaconStore, PathDatabase};
use crate::types::RouterId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// AS-level SCION control service
///
/// One instance per ISD-AS, shared by all routers in that AS.
/// Manages external interfaces, border routers, and beaconing state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScionControlService {
    /// ISD-AS identifier for this AS
    pub isd_as: IsdAs,

    /// Whether this AS is a core AS
    pub is_core: bool,

    /// Shared beacon store for all routers in this AS
    #[serde(skip)]
    pub beacon_store: BeaconStore,

    /// Shared path database for all routers in this AS
    #[serde(skip)]
    pub path_db: PathDatabase,

    /// Border routers in this AS (routers with external SCION interfaces)
    pub border_routers: Vec<RouterId>,

    /// SCION interfaces (external links only)
    /// Maps interface ID to interface information
    pub interfaces: HashMap<InterfaceId, InterfaceInfo>,

    /// Next interface ID to allocate
    next_interface_id: u16,
}

/// Information about a SCION interface (external link)
///
/// Each interface represents one end of a link between two ASes.
/// Interface IDs are AS-global and unique within the AS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceInfo {
    /// Which border router has this interface
    pub local_router: RouterId,

    /// Neighbor AS connected via this interface
    pub remote_as: IsdAs,

    /// Interface ID on the remote AS side
    pub remote_interface: InterfaceId,

    /// Type of link (Parent, Child, Core, Peer)
    pub link_type: ScionLinkType,
}

impl ScionControlService {
    /// Create a new SCION control service for an AS
    pub fn new(isd_as: IsdAs, is_core: bool) -> Self {
        Self {
            isd_as,
            is_core,
            beacon_store: BeaconStore::new(),
            path_db: PathDatabase::new(),
            border_routers: Vec::new(),
            interfaces: HashMap::new(),
            next_interface_id: 1, // Interface ID 0 is reserved
        }
    }

    /// Allocate a new interface ID
    ///
    /// Returns the next available interface ID and increments the counter.
    pub fn allocate_interface(&mut self) -> InterfaceId {
        let id = InterfaceId::new(self.next_interface_id);
        self.next_interface_id += 1;
        id
    }

    /// Add a SCION interface
    ///
    /// This registers an external link with a neighbor AS.
    pub fn add_interface(&mut self, iface_id: InterfaceId, info: InterfaceInfo) {
        // Add border router if not already tracked
        if !self.border_routers.contains(&info.local_router) {
            self.border_routers.push(info.local_router);
        }

        self.interfaces.insert(iface_id, info);
    }

    /// Get all interfaces of a specific link type
    fn get_interfaces_by_type(&self, link_type: ScionLinkType) -> Vec<(InterfaceId, &InterfaceInfo)> {
        self.interfaces
            .iter()
            .filter(|(_, info)| info.link_type == link_type)
            .map(|(id, info)| (*id, info))
            .collect()
    }

    /// Get all child interfaces (for propagating PCBs downward)
    pub fn get_child_interfaces(&self) -> Vec<(IsdAs, InterfaceId)> {
        self.get_interfaces_by_type(ScionLinkType::Child)
            .into_iter()
            .map(|(id, info)| (info.remote_as, id))
            .collect()
    }

    /// Get all parent interfaces (for propagating PCBs upward)
    pub fn get_parent_interfaces(&self) -> Vec<(IsdAs, InterfaceId)> {
        self.get_interfaces_by_type(ScionLinkType::Parent)
            .into_iter()
            .map(|(id, info)| (info.remote_as, id))
            .collect()
    }

    /// Get all core interfaces (for propagating PCBs to core neighbors)
    pub fn get_core_interfaces(&self) -> Vec<(IsdAs, InterfaceId)> {
        self.get_interfaces_by_type(ScionLinkType::Core)
            .into_iter()
            .map(|(id, info)| (info.remote_as, id))
            .collect()
    }

    /// Get all peering interfaces
    pub fn get_peer_interfaces(&self) -> Vec<(IsdAs, InterfaceId)> {
        self.get_interfaces_by_type(ScionLinkType::Peer)
            .into_iter()
            .map(|(id, info)| (info.remote_as, id))
            .collect()
    }

    /// Check if a router is a border router
    pub fn is_border_router(&self, router: RouterId) -> bool {
        self.border_routers.contains(&router)
    }

    /// Get the interface ID for a link to a specific neighbor AS
    ///
    /// Returns the first matching interface. If multiple interfaces exist to the same
    /// neighbor AS, this returns an arbitrary one.
    pub fn get_interface_to(&self, neighbor_as: IsdAs) -> Option<InterfaceId> {
        self.interfaces
            .iter()
            .find(|(_, info)| info.remote_as == neighbor_as)
            .map(|(id, _)| *id)
    }

    /// Get all interfaces to a specific neighbor AS
    pub fn get_interfaces_to(&self, neighbor_as: IsdAs) -> Vec<InterfaceId> {
        self.interfaces
            .iter()
            .filter(|(_, info)| info.remote_as == neighbor_as)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get interface info by ID
    pub fn get_interface(&self, iface_id: InterfaceId) -> Option<&InterfaceInfo> {
        self.interfaces.get(&iface_id)
    }

    // ===== PCB Validation Methods (§2.3.1) =====

    /// Validate an incoming PCB
    ///
    /// Performs all required validation checks from §2.3.1:
    /// - Loop detection
    /// - Incoming interface validation
    /// - Link type validation
    /// - Continuity check
    pub fn validate_pcb(
        &self,
        pcb: &super::pcb::Pcb,
        link_type: ScionLinkType,
        receiving_iface: Option<InterfaceId>,
    ) -> bool {
        // Basic check: PCB must not be empty
        if pcb.is_empty() {
            return false;
        }

        // Loop detection (§2.3.1)
        if self.has_loop(pcb) {
            return false;
        }

        // Link type validation (§2.3.1)
        if !self.validate_link_type(link_type) {
            return false;
        }

        // Incoming interface validation (§2.3.1)
        if let Some(iface_id) = receiving_iface {
            if !self.validate_incoming_interface(pcb, iface_id) {
                return false;
            }
        }

        // Continuity check (§2.3.1)
        if !self.validate_continuity(pcb) {
            return false;
        }

        true
    }

    /// Check if this AS already appears in the PCB (loop detection)
    ///
    /// §2.3.1: Core ASes MUST check for duplicate hop entries.
    /// PCBs with loops MUST be discarded.
    fn has_loop(&self, pcb: &super::pcb::Pcb) -> bool {
        pcb.contains(self.isd_as)
    }

    /// Validate that the last AS entry in PCB matches the neighbor on the receiving interface
    ///
    /// §2.3.1: The last ISD-AS entry in the received PCB MUST match the ISD-AS
    /// neighbor of the interface where the PCB was received.
    fn validate_incoming_interface(
        &self,
        pcb: &super::pcb::Pcb,
        receiving_iface: InterfaceId,
    ) -> bool {
        // Get last AS entry
        let last_entry = match pcb.as_entries.last() {
            Some(entry) => entry,
            None => return false,
        };

        // Get interface info
        let iface_info = match self.interfaces.get(&receiving_iface) {
            Some(info) => info,
            None => return false,
        };

        // Last AS entry must match neighbor on this interface
        last_entry.isd_as == iface_info.remote_as
    }

    /// Validate link type is appropriate for beaconing mode
    ///
    /// §2.3.1: The corresponding link MUST be core or parent (not peering).
    fn validate_link_type(&self, link_type: ScionLinkType) -> bool {
        match link_type {
            // Core beaconing: accept from Core links
            ScionLinkType::Core if self.is_core => true,

            // Intra-ISD beaconing: accept from Parent links
            ScionLinkType::Parent if !self.is_core => true,

            // Core AS can also receive intra-ISD from children (origination case)
            ScionLinkType::Child if self.is_core => true,

            // Never accept from Peer links
            ScionLinkType::Peer => false,

            // All other combinations invalid
            _ => false,
        }
    }

    /// Verify continuity: each AS entry's next_isd_as equals next entry's isd_as
    ///
    /// §2.3.1: When a PCB contains two or more AS entries, the receiver MUST check
    /// that every AS entry (except the last) has an ISD-AS that equals the ISD-AS
    /// of the next entry.
    fn validate_continuity(&self, pcb: &super::pcb::Pcb) -> bool {
        let entries = &pcb.as_entries;

        if entries.len() < 2 {
            return true; // Single entry or empty PCB (handled elsewhere)
        }

        for i in 0..entries.len() - 1 {
            if let Some(next_as) = entries[i].next_isd_as {
                if next_as != entries[i + 1].isd_as {
                    return false; // Continuity violation
                }
            } else {
                // Entry has no next_isd_as but is not the last entry
                return false;
            }
        }

        true
    }

    // ===== PCB Selection Methods (§2.3.3) =====

    /// Select best N PCBs by shortest AS path length
    ///
    /// §2.3.3: AS path length is the primary selection criterion.
    /// This method selects the N shortest PCBs from the beacon store.
    ///
    /// # Arguments
    /// * `limit` - Maximum number of PCBs to select (≤50 for intra-ISD, ≤5 for core)
    pub fn select_best_pcbs(&self, limit: usize) -> Vec<std::sync::Arc<super::pcb::Pcb>> {
        use std::cmp::Ordering;

        let mut pcbs: Vec<_> = self.beacon_store.get_all().collect();

        // Sort by AS path length (shorter is better)
        pcbs.sort_by(|a, b| {
            match a.len().cmp(&b.len()) {
                Ordering::Equal => {
                    // Tie-break by segment_id for determinism
                    a.segment_info.segment_id.cmp(&b.segment_info.segment_id)
                }
                other => other,
            }
        });

        // Take top N
        pcbs.into_iter().take(limit).collect()
    }

    /// Select best N PCBs for a specific neighbor (core beaconing)
    ///
    /// §3.4.2: For core beaconing, select ≤5 PCBs per immediate neighbor core AS.
    /// This method filters out PCBs that already contain the neighbor (loop prevention)
    /// and selects the shortest remaining paths.
    ///
    /// # Arguments
    /// * `neighbor_as` - The neighbor AS we're propagating to
    /// * `limit` - Maximum number of PCBs to select (typically 5 for core)
    pub fn select_best_pcbs_for_neighbor(
        &self,
        neighbor_as: IsdAs,
        limit: usize,
    ) -> Vec<std::sync::Arc<super::pcb::Pcb>> {
        use std::cmp::Ordering;

        // Filter out PCBs that already contain the neighbor (loop prevention)
        let mut pcbs: Vec<_> = self
            .beacon_store
            .get_all()
            .filter(|pcb| !pcb.contains(neighbor_as))
            .collect();

        // Sort by AS path length
        pcbs.sort_by(|a, b| {
            match a.len().cmp(&b.len()) {
                Ordering::Equal => {
                    a.segment_info.segment_id.cmp(&b.segment_info.segment_id)
                }
                other => other,
            }
        });

        // Take top N
        pcbs.into_iter().take(limit).collect()
    }

    // ===== PCB Extension and Origination Methods (§2.3.5) =====

    /// Determine the ingress interface for a received PCB
    ///
    /// This finds which interface the PCB came from by matching the last AS entry's
    /// egress interface with our interface that connects to that AS.
    fn get_ingress_interface(&self, pcb: &super::pcb::Pcb) -> Option<InterfaceId> {
        // Get the last AS entry
        let last_entry = pcb.as_entries.last()?;

        // Get the egress interface from the last hop
        let remote_egress = last_entry.hop_entry.egress?;

        // Find our interface that connects to the last AS
        for (iface_id, iface_info) in &self.interfaces {
            if iface_info.remote_as == last_entry.isd_as
                && iface_info.remote_interface == remote_egress
            {
                return Some(*iface_id);
            }
        }

        None
    }

    /// Extend PCB for intra-ISD propagation to child AS
    ///
    /// §2.3.5.1: For intra-ISD beaconing, MUST add a new AS entry including:
    /// - Hop Entry with ingress/egress interface IDs
    /// - Any Peer Entry information (currently not implemented)
    ///
    /// # Arguments
    /// * `pcb` - The PCB to extend (Arc for efficiency)
    /// * `egress_iface` - Our egress interface to the child AS
    /// * `next_as` - The child AS we're propagating to
    pub fn extend_pcb_intra_isd(
        &self,
        pcb: std::sync::Arc<super::pcb::Pcb>,
        egress_iface: InterfaceId,
        next_as: IsdAs,
    ) -> std::sync::Arc<super::pcb::Pcb> {
        use super::pcb::{AsEntry, HopEntry};

        let mut new_pcb = (*pcb).clone();

        // Determine ingress interface (where we received this PCB)
        let ingress_iface = self
            .get_ingress_interface(&pcb)
            .unwrap_or(InterfaceId::ZERO); // ZERO if we originated it

        // Create hop entry with ingress/egress
        let hop_entry = HopEntry::new(ingress_iface, Some(egress_iface));

        // Create AS entry
        let as_entry = AsEntry::new(self.isd_as, Some(next_as), hop_entry);

        // Note: Peer entries could be added here in the future
        // for peer in self.get_configured_peers() {
        //     as_entry.add_peer_entry(peer);
        // }

        new_pcb.extend(as_entry);
        std::sync::Arc::new(new_pcb)
    }

    /// Extend PCB for core propagation to neighboring core AS
    ///
    /// §2.3.5.2: For core beaconing, MUST add a new AS entry which MUST include:
    /// - The egress interface to the neighboring core AS in the Hop Field
    /// - The ISD-AS number of the neighboring core AS
    ///
    /// # Arguments
    /// * `pcb` - The PCB to extend (Arc for efficiency)
    /// * `egress_iface` - Our egress interface to the core neighbor
    /// * `next_as` - The neighboring core AS
    pub fn extend_pcb_core(
        &self,
        pcb: std::sync::Arc<super::pcb::Pcb>,
        egress_iface: InterfaceId,
        next_as: IsdAs,
    ) -> std::sync::Arc<super::pcb::Pcb> {
        use super::pcb::{AsEntry, HopEntry};

        let mut new_pcb = (*pcb).clone();

        // Determine ingress interface
        let ingress_iface = self
            .get_ingress_interface(&pcb)
            .unwrap_or(InterfaceId::ZERO);

        // Create hop entry
        let hop_entry = HopEntry::new(ingress_iface, Some(egress_iface));

        // Create AS entry with neighbor core AS
        let as_entry = AsEntry::new(self.isd_as, Some(next_as), hop_entry);

        new_pcb.extend(as_entry);
        std::sync::Arc::new(new_pcb)
    }

    /// Create initial PCB originating from this core AS
    ///
    /// Core ASes create initial PCBs for both intra-ISD and core beaconing.
    /// The originating AS has ingress = InterfaceId::ZERO to indicate it's the source.
    ///
    /// # Arguments
    /// * `egress_iface` - Our egress interface to the neighbor
    /// * `next_as` - The neighboring AS (child for intra-ISD, core for core beaconing)
    pub fn originate_pcb(
        &self,
        egress_iface: InterfaceId,
        next_as: IsdAs,
    ) -> std::sync::Arc<super::pcb::Pcb> {
        use super::pcb::{AsEntry, HopEntry, Pcb};

        // Create empty PCB
        let mut pcb = Pcb::new(self.isd_as);

        // Create initial AS entry (ingress = 0 for originator)
        let hop_entry = HopEntry::new(
            InterfaceId::ZERO, // No ingress (we're the origin)
            Some(egress_iface),
        );

        let as_entry = AsEntry::new(self.isd_as, Some(next_as), hop_entry);

        pcb.extend(as_entry);
        std::sync::Arc::new(pcb)
    }

    // ===== PCB Propagation Methods =====

    /// Propagate intra-ISD PCBs to children
    ///
    /// This method handles both PCB origination (for core ASes) and forwarding (for non-core ASes).
    /// Creates a vector of (remote_as, Vec<Arc<Pcb>>) tuples representing BeaconBatch events.
    ///
    /// §3.4.1: Propagate ≤50 PCBs per child link (typical: ~20)
    pub fn propagate_intra_isd_pcbs(
        &mut self,
    ) -> Vec<(IsdAs, Vec<std::sync::Arc<super::pcb::Pcb>>)> {
        let mut batches = Vec::new();

        // For each child interface
        for (iface_id, iface_info) in &self.interfaces {
            if iface_info.link_type != ScionLinkType::Child {
                continue;
            }

            let pcbs = if self.is_core {
                // Core AS: originate one PCB per child interface
                vec![self.originate_pcb(*iface_id, iface_info.remote_as)]
            } else {
                // Non-core AS: select and extend received PCBs
                let selected = self.select_best_pcbs(50); // ≤50 per child (§3.4.1)

                selected
                    .into_iter()
                    .map(|pcb| self.extend_pcb_intra_isd(pcb, *iface_id, iface_info.remote_as))
                    .collect()
            };

            if !pcbs.is_empty() {
                batches.push((iface_info.remote_as, pcbs));
            }
        }

        batches
    }

    /// Propagate core PCBs to neighboring core ASes
    ///
    /// §3.4.2: Propagate ≤5 PCBs per immediate neighbor core AS
    pub fn propagate_core_pcbs(&mut self) -> Vec<(IsdAs, Vec<std::sync::Arc<super::pcb::Pcb>>)> {
        let mut batches = Vec::new();

        // For each core interface
        for (iface_id, iface_info) in &self.interfaces {
            if iface_info.link_type != ScionLinkType::Core {
                continue;
            }

            // If beacon store is empty, originate PCBs
            let pcbs = if self.beacon_store.get_all().count() == 0 {
                // Originate one PCB for this neighbor
                vec![self.originate_pcb(*iface_id, iface_info.remote_as)]
            } else {
                // Select best PCBs for this neighbor (≤5 per neighbor, §3.4.2)
                let selected = self.select_best_pcbs_for_neighbor(iface_info.remote_as, 5);

                // Extend each PCB
                selected
                    .into_iter()
                    .map(|pcb| self.extend_pcb_core(pcb, *iface_id, iface_info.remote_as))
                    .collect()
            };

            if !pcbs.is_empty() {
                batches.push((iface_info.remote_as, pcbs));
            }
        }

        batches
    }

    // ===== PCB Termination and Segment Registration (§4) =====

    /// Terminate a PCB by adding a final AS entry
    ///
    /// §4.1.1: Termination converts a traveling PCB into a static path segment.
    /// The final AS entry MUST have:
    /// - next_isd_as = None (MUST NOT be specified)
    /// - egress = None in hop field (MUST NOT be specified)
    ///
    /// # Arguments
    /// * `pcb` - The PCB to terminate
    /// * `segment_type` - The type of segment being created (Up/Down/Core)
    fn terminate_pcb(
        &self,
        pcb: std::sync::Arc<super::pcb::Pcb>,
        segment_type: super::path_db::SegmentType,
    ) -> std::sync::Arc<super::path_db::PathSegment> {
        use super::pcb::{AsEntry, HopEntry};

        let mut new_pcb = (*pcb).clone();

        // Determine ingress interface (where the PCB entered this AS)
        let ingress_iface = self
            .get_ingress_interface(&pcb)
            .unwrap_or(InterfaceId::ZERO);

        // Create terminal hop entry (no egress)
        let hop_entry = HopEntry::new(
            ingress_iface,
            None, // No egress - this is the end
        );

        // Create terminal AS entry (no next_isd_as)
        let as_entry = AsEntry::new(
            self.isd_as,
            None, // No next AS - this is the end
            hop_entry,
        );

        // Note: Peer entries could be added here in the future (§4.1.1)
        // for peer in self.get_configured_peers() {
        //     as_entry.add_peer_entry(peer);
        // }

        new_pcb.extend(as_entry);

        // Create path segment
        std::sync::Arc::new(super::path_db::PathSegment::new(segment_type, new_pcb))
    }

    /// Register up segments (non-core AS only)
    ///
    /// §4.1.2: Non-core ASes transform selected PCBs into up segments and store
    /// them locally in their path database.
    ///
    /// Returns the registered segments for inspection/testing.
    pub fn register_up_segments(&mut self) -> Vec<std::sync::Arc<super::path_db::PathSegment>> {
        use super::path_db::SegmentType;

        if self.is_core {
            return vec![]; // Core ASes don't register up segments
        }

        // Select best PCBs to transform into up segments
        // Typical: ~20 up segments per AS
        let pcbs = self.select_best_pcbs(20);

        let mut segments = Vec::new();

        for pcb in pcbs {
            // Terminate PCB as up segment
            let segment = self.terminate_pcb(pcb, SegmentType::Up);

            // Store in local path database
            self.path_db.add_segment(segment.clone());

            segments.push(segment);
        }

        segments
    }

    /// Register down segments (non-core AS only)
    ///
    /// §4.1.3: Non-core ASes transform selected PCBs into down segments and
    /// register them with the originating core ASes.
    ///
    /// Returns (core_as, segments) tuples for creating SegmentRegistration events.
    pub fn register_down_segments(
        &mut self,
    ) -> Vec<(IsdAs, Vec<std::sync::Arc<super::path_db::PathSegment>>)> {
        use super::path_db::SegmentType;
        use std::collections::HashMap;

        if self.is_core {
            return vec![]; // Core ASes don't register down segments
        }

        // Select best PCBs to transform into down segments
        // Typical: ~20 down segments (can differ from up segments)
        let pcbs = self.select_best_pcbs(20);

        // Group by originating core AS
        let mut batches: HashMap<IsdAs, Vec<std::sync::Arc<super::path_db::PathSegment>>> =
            HashMap::new();

        for pcb in pcbs {
            // Get originating core AS (first AS in PCB)
            if let Some(origin_as) = pcb.src() {
                // Terminate PCB as down segment
                let segment = self.terminate_pcb(pcb, SegmentType::Down);

                batches
                    .entry(origin_as)
                    .or_insert_with(Vec::new)
                    .push(segment);
            }
        }

        batches.into_iter().collect()
    }

    /// Register core segments (core AS only)
    ///
    /// §4.2: Core ASes transform selected PCBs into core segments and store
    /// them locally. No need to send to other core ASes - each will receive
    /// PCBs from all others during beaconing.
    ///
    /// Returns the registered segments for inspection/testing.
    pub fn register_core_segments(
        &mut self,
    ) -> Vec<std::sync::Arc<super::path_db::PathSegment>> {
        use super::path_db::SegmentType;

        if !self.is_core {
            return vec![]; // Only core ASes register core segments
        }

        // Select best PCBs toward each observed core AS
        // More diversity for core (up to 50)
        let pcbs = self.select_best_pcbs(50);

        let mut segments = Vec::new();

        for pcb in pcbs {
            // Terminate PCB as core segment
            let segment = self.terminate_pcb(pcb, SegmentType::Core);

            // Store in local path database
            self.path_db.add_segment(segment.clone());

            segments.push(segment);
        }

        segments
    }

    /// Validate a down segment for registration
    ///
    /// §4.1.3: The first ISD-AS entry of the path segment SHOULD equal the core
    /// ISD-AS where the segment is being registered. If not, MUST reject.
    pub fn validate_down_segment(&self, segment: &super::path_db::PathSegment) -> bool {
        // Get first AS entry from the PCB
        let first_as = segment.pcb.src();

        // Must equal our ISD-AS
        first_as == Some(self.isd_as)
    }

    /// Query for paths to a destination AS
    ///
    /// §5: Path lookup combines up, core, and down segments to create
    /// end-to-end paths. Optionally includes peering shortcuts.
    ///
    /// This method constructs paths from this AS to the destination AS
    /// by combining segments from the path database.
    pub fn query_paths(
        &self,
        dst: IsdAs,
        max_paths: usize,
        allow_peering: bool,
    ) -> super::path_query::PathQueryResult {
        use super::path_construction::construct_paths_with_peering;
        use super::path_query::PathQuery;

        let query = PathQuery {
            src: self.isd_as,
            dst,
            max_paths,
            allow_peering,
        };

        // Use path construction algorithm
        construct_paths_with_peering(&query, &self.path_db, self.is_core, false)
        // Note: We assume dst_is_core=false for simplicity
        // In a full implementation, we'd need to track which ASes are core
    }

    /// Query for paths with a full PathQuery object
    ///
    /// This is a more flexible version of query_paths that accepts
    /// a pre-built PathQuery object.
    pub fn query_paths_full(
        &self,
        query: &super::path_query::PathQuery,
    ) -> super::path_query::PathQueryResult {
        use super::path_construction::construct_paths_with_peering;

        construct_paths_with_peering(query, &self.path_db, self.is_core, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_control_service_creation() {
        let isd_as = IsdAs::new(1, 100);
        let cs = ScionControlService::new(isd_as, true);

        assert_eq!(cs.isd_as, isd_as);
        assert!(cs.is_core);
        assert!(cs.border_routers.is_empty());
        assert!(cs.interfaces.is_empty());
    }

    #[test]
    fn test_interface_allocation() {
        let mut cs = ScionControlService::new(IsdAs::new(1, 100), false);

        let id1 = cs.allocate_interface();
        let id2 = cs.allocate_interface();
        let id3 = cs.allocate_interface();

        assert_eq!(id1.as_u16(), 1);
        assert_eq!(id2.as_u16(), 2);
        assert_eq!(id3.as_u16(), 3);
    }

    #[test]
    fn test_add_interface() {
        let mut cs = ScionControlService::new(IsdAs::new(1, 100), true);

        let router = RouterId::from(0);
        let iface_id = cs.allocate_interface();
        let info = InterfaceInfo {
            local_router: router,
            remote_as: IsdAs::new(1, 101),
            remote_interface: InterfaceId::new(1),
            link_type: ScionLinkType::Child,
        };

        cs.add_interface(iface_id, info.clone());

        assert_eq!(cs.interfaces.len(), 1);
        assert_eq!(cs.border_routers.len(), 1);
        assert!(cs.border_routers.contains(&router));
        assert_eq!(cs.get_interface(iface_id), Some(&info));
    }

    #[test]
    fn test_get_interfaces_by_type() {
        let mut cs = ScionControlService::new(IsdAs::new(1, 100), true);

        // Add child interface
        let child_iface = cs.allocate_interface();
        cs.add_interface(
            child_iface,
            InterfaceInfo {
                local_router: RouterId::from(0),
                remote_as: IsdAs::new(1, 101),
                remote_interface: InterfaceId::new(1),
                link_type: ScionLinkType::Child,
            },
        );

        // Add core interface
        let core_iface = cs.allocate_interface();
        cs.add_interface(
            core_iface,
            InterfaceInfo {
                local_router: RouterId::from(1),
                remote_as: IsdAs::new(1, 102),
                remote_interface: InterfaceId::new(1),
                link_type: ScionLinkType::Core,
            },
        );

        // Add another child interface
        let child_iface2 = cs.allocate_interface();
        cs.add_interface(
            child_iface2,
            InterfaceInfo {
                local_router: RouterId::from(2),
                remote_as: IsdAs::new(1, 103),
                remote_interface: InterfaceId::new(1),
                link_type: ScionLinkType::Child,
            },
        );

        let child_interfaces = cs.get_child_interfaces();
        assert_eq!(child_interfaces.len(), 2);
        assert!(child_interfaces.contains(&(IsdAs::new(1, 101), child_iface)));
        assert!(child_interfaces.contains(&(IsdAs::new(1, 103), child_iface2)));

        let core_interfaces = cs.get_core_interfaces();
        assert_eq!(core_interfaces.len(), 1);
        assert!(core_interfaces.contains(&(IsdAs::new(1, 102), core_iface)));

        let parent_interfaces = cs.get_parent_interfaces();
        assert_eq!(parent_interfaces.len(), 0);
    }

    #[test]
    fn test_is_border_router() {
        let mut cs = ScionControlService::new(IsdAs::new(1, 100), false);

        let r1 = RouterId::from(0);
        let r2 = RouterId::from(1);
        let r3 = RouterId::from(2);

        // Add interface on r1
        let iface = cs.allocate_interface();
        cs.add_interface(
            iface,
            InterfaceInfo {
                local_router: r1,
                remote_as: IsdAs::new(1, 101),
                remote_interface: InterfaceId::new(1),
                link_type: ScionLinkType::Parent,
            },
        );

        assert!(cs.is_border_router(r1));
        assert!(!cs.is_border_router(r2));
        assert!(!cs.is_border_router(r3));

        // Add interface on r2
        let iface2 = cs.allocate_interface();
        cs.add_interface(
            iface2,
            InterfaceInfo {
                local_router: r2,
                remote_as: IsdAs::new(1, 102),
                remote_interface: InterfaceId::new(1),
                link_type: ScionLinkType::Parent,
            },
        );

        assert!(cs.is_border_router(r1));
        assert!(cs.is_border_router(r2));
        assert!(!cs.is_border_router(r3));
    }

    #[test]
    fn test_get_interface_to() {
        let mut cs = ScionControlService::new(IsdAs::new(1, 100), true);

        let neighbor = IsdAs::new(1, 101);
        let iface = cs.allocate_interface();
        cs.add_interface(
            iface,
            InterfaceInfo {
                local_router: RouterId::from(0),
                remote_as: neighbor,
                remote_interface: InterfaceId::new(1),
                link_type: ScionLinkType::Child,
            },
        );

        assert_eq!(cs.get_interface_to(neighbor), Some(iface));
        assert_eq!(cs.get_interface_to(IsdAs::new(1, 102)), None);
    }

    #[test]
    fn test_get_interfaces_to_multiple() {
        let mut cs = ScionControlService::new(IsdAs::new(1, 100), true);

        let neighbor = IsdAs::new(1, 101);

        // Add two interfaces to the same neighbor
        let iface1 = cs.allocate_interface();
        cs.add_interface(
            iface1,
            InterfaceInfo {
                local_router: RouterId::from(0),
                remote_as: neighbor,
                remote_interface: InterfaceId::new(1),
                link_type: ScionLinkType::Child,
            },
        );

        let iface2 = cs.allocate_interface();
        cs.add_interface(
            iface2,
            InterfaceInfo {
                local_router: RouterId::from(1),
                remote_as: neighbor,
                remote_interface: InterfaceId::new(2),
                link_type: ScionLinkType::Child,
            },
        );

        let interfaces = cs.get_interfaces_to(neighbor);
        assert_eq!(interfaces.len(), 2);
        assert!(interfaces.contains(&iface1));
        assert!(interfaces.contains(&iface2));
    }
}
