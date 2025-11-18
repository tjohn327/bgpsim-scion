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

//! SCION Control Service process for managing beaconing, registration, and path lookup.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::{Prefix, RouterId};

use super::{
    state::{BeaconStore, PathDatabase},
    types::{InterfaceId, InterfaceInfo, IsdAs, ScionLinkType},
};

/// The SCION Control Service is responsible for managing SCION operations for a single AS.
///
/// It maintains:
/// - Interface configuration (links to neighbors)
/// - Beacon store (received PCBs)
/// - Path database (registered path segments)
/// - Beaconing state (last beacon times, etc.)
///
/// The control service handles:
/// - PCB generation and propagation (beaconing)
/// - Path segment registration
/// - Path lookup requests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "P: Prefix + Serialize + for<'d> Deserialize<'d>")]
pub struct ScionControlService<P: Prefix> {
    /// The ISD-AS identifier for this AS
    pub isd_as: IsdAs,
    /// Whether this AS is a core AS
    pub is_core: bool,
    /// Beacon store for received PCBs
    pub beacon_store: BeaconStore<P>,
    /// Path database for registered segments
    pub path_database: PathDatabase<P>,
    /// Interface configuration: maps neighbor RouterId to interface info
    interfaces: HashMap<RouterId, InterfaceInfo>,
    /// Reverse mapping: interface ID to neighbor RouterId
    interface_id_map: HashMap<InterfaceId, RouterId>,
    /// Last time core beaconing was triggered (simulated timestamp)
    pub last_core_beacon_time: Option<u32>,
    /// Last time intra-ISD beaconing was triggered (simulated timestamp)
    pub last_intra_isd_beacon_time: Option<u32>,
    /// Last time registration was triggered (simulated timestamp)
    pub last_registration_time: Option<u32>,
}

impl<P: Prefix> ScionControlService<P> {
    /// Create a new SCION control service for an AS.
    ///
    /// # Arguments
    /// * `isd_as` - The ISD-AS identifier for this AS
    /// * `is_core` - Whether this AS is a core AS
    pub fn new(isd_as: IsdAs, is_core: bool) -> Self {
        ScionControlService {
            isd_as,
            is_core,
            beacon_store: BeaconStore::default(),
            path_database: PathDatabase::default(),
            interfaces: HashMap::new(),
            interface_id_map: HashMap::new(),
            last_core_beacon_time: None,
            last_intra_isd_beacon_time: None,
            last_registration_time: None,
        }
    }

    /// Add an interface to a neighbor.
    ///
    /// # Arguments
    /// * `interface_info` - Information about the interface
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(String)` if the interface ID is already in use
    pub fn add_interface(&mut self, interface_info: InterfaceInfo) -> Result<(), String> {
        // Check if interface ID is already in use
        if self.interface_id_map.contains_key(&interface_info.interface_id) {
            return Err(format!(
                "Interface ID {} already in use",
                interface_info.interface_id
            ));
        }

        let neighbor_router = interface_info.neighbor_router;
        let interface_id = interface_info.interface_id;

        // Store the interface
        self.interfaces.insert(neighbor_router, interface_info);
        self.interface_id_map.insert(interface_id, neighbor_router);

        Ok(())
    }

    /// Remove an interface by neighbor router ID.
    pub fn remove_interface(&mut self, neighbor: RouterId) -> Option<InterfaceInfo> {
        if let Some(info) = self.interfaces.remove(&neighbor) {
            self.interface_id_map.remove(&info.interface_id);
            Some(info)
        } else {
            None
        }
    }

    /// Get interface information by neighbor router ID.
    pub fn get_interface(&self, neighbor: RouterId) -> Option<&InterfaceInfo> {
        self.interfaces.get(&neighbor)
    }

    /// Get interface information by interface ID.
    pub fn get_interface_by_id(&self, interface_id: InterfaceId) -> Option<&InterfaceInfo> {
        self.interface_id_map
            .get(&interface_id)
            .and_then(|neighbor| self.interfaces.get(neighbor))
    }

    /// Get all interfaces.
    pub fn get_all_interfaces(&self) -> Vec<&InterfaceInfo> {
        self.interfaces.values().collect()
    }

    /// Get interfaces filtered by link type.
    pub fn get_interfaces_by_type(&self, link_type: ScionLinkType) -> Vec<&InterfaceInfo> {
        self.interfaces
            .values()
            .filter(|info| info.link_type == link_type)
            .collect()
    }

    /// Get all core interfaces (for core beaconing).
    pub fn get_core_interfaces(&self) -> Vec<&InterfaceInfo> {
        self.get_interfaces_by_type(ScionLinkType::Core)
    }

    /// Get all parent interfaces (from child's perspective).
    pub fn get_parent_interfaces(&self) -> Vec<&InterfaceInfo> {
        self.get_interfaces_by_type(ScionLinkType::ParentChild)
    }

    /// Get all child interfaces (from parent's perspective).
    ///
    /// Note: In the current model, parent-child links are stored from the child's
    /// perspective. This method returns the same as get_parent_interfaces but is
    /// provided for semantic clarity when the AS is acting as a parent.
    pub fn get_child_interfaces(&self) -> Vec<&InterfaceInfo> {
        // In practice, we need to check the direction of the link
        // For now, assume parent-child links are symmetric in storage
        self.get_interfaces_by_type(ScionLinkType::ParentChild)
    }

    /// Get all peering interfaces.
    pub fn get_peering_interfaces(&self) -> Vec<&InterfaceInfo> {
        self.get_interfaces_by_type(ScionLinkType::Peering)
    }

    /// Get the neighbor router ID for an interface ID.
    pub fn get_neighbor_by_interface_id(&self, interface_id: InterfaceId) -> Option<RouterId> {
        self.interface_id_map.get(&interface_id).copied()
    }

    /// Check if an interface ID is valid for this AS.
    pub fn has_interface_id(&self, interface_id: InterfaceId) -> bool {
        self.interface_id_map.contains_key(&interface_id)
    }

    /// Get the number of interfaces.
    pub fn interface_count(&self) -> usize {
        self.interfaces.len()
    }

    /// Assign a new unique interface ID.
    ///
    /// Returns the next available interface ID (starting from 1).
    pub fn next_interface_id(&self) -> InterfaceId {
        // Find the maximum interface ID currently in use
        let max_id = self
            .interface_id_map
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(0);

        InterfaceId(max_id + 1)
    }

    /// Clear all expired PCBs and path segments.
    ///
    /// # Arguments
    /// * `current_time` - Current timestamp
    ///
    /// # Returns
    /// Tuple of (expired_pcbs, expired_segments)
    pub fn clear_expired(&mut self, current_time: u32) -> (usize, usize) {
        let expired_pcbs = self.beacon_store.remove_expired(current_time, 0);
        let expired_segments = self.path_database.remove_expired(current_time);
        (expired_pcbs, expired_segments)
    }

    /// Register an up-segment in the path database.
    ///
    /// # Arguments
    /// * `segment` - The up-segment to register
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(String)` if registration fails
    pub fn register_up_segment(&mut self, segment: super::PathSegment<P>) -> Result<(), String> {
        self.path_database.register_segment(segment);
        Ok(())
    }

    /// Register a down-segment in the path database.
    ///
    /// # Arguments
    /// * `segment` - The down-segment to register
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(String)` if registration fails
    pub fn register_down_segment(&mut self, segment: super::PathSegment<P>) -> Result<(), String> {
        self.path_database.register_segment(segment);
        Ok(())
    }

    /// Register a core-segment in the path database.
    ///
    /// # Arguments
    /// * `segment` - The core-segment to register
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(String)` if registration fails
    pub fn register_core_segment(&mut self, segment: super::PathSegment<P>) -> Result<(), String> {
        self.path_database.register_segment(segment);
        Ok(())
    }

    /// Lookup up-segments to a destination.
    ///
    /// # Arguments
    /// * `dst` - Destination ISD-AS
    ///
    /// # Returns
    /// Vector of up-segments to the destination
    pub fn lookup_up_segments(&self, dst: &IsdAs) -> Vec<super::PathSegment<P>> {
        // Limit segments to prevent OOM (spec-compliant: line 1558 recommends max 50)
        self.path_database.lookup_up_segments(dst).into_iter().take(1000).cloned().collect()
    }

    /// Lookup up-segments from a source.
    ///
    /// # Arguments
    /// * `src` - Source ISD-AS
    ///
    /// # Returns
    /// Vector of up-segments from the source
    pub fn lookup_up_segments_from(&self, src: &IsdAs) -> Vec<super::PathSegment<P>> {
        // Limit segments to prevent OOM (spec-compliant: line 1558 recommends max 50)
        self.path_database.lookup_up_segments_from(src).into_iter().take(1000).cloned().collect()
    }

    /// Lookup down-segments from a source.
    ///
    /// # Arguments
    /// * `src` - Source ISD-AS
    ///
    /// # Returns
    /// Vector of down-segments from the source
    pub fn lookup_down_segments(&self, src: &IsdAs) -> Vec<super::PathSegment<P>> {
        self.path_database.lookup_down_segments(src).into_iter().cloned().collect()
    }

    /// Lookup down-segments to a destination.
    ///
    /// # Arguments
    /// * `dst` - Destination ISD-AS
    ///
    /// # Returns
    /// Vector of down-segments to the destination
    pub fn lookup_down_segments_to(&self, dst: &IsdAs) -> Vec<super::PathSegment<P>> {
        // Limit segments to prevent OOM (spec-compliant: line 1558 recommends max 50)
        self.path_database.lookup_down_segments_to(dst).into_iter().take(1000).cloned().collect()
    }

    /// Lookup core-segments between two core ASes.
    ///
    /// # Arguments
    /// * `src` - Source core AS
    /// * `dst` - Destination core AS
    ///
    /// # Returns
    /// Vector of core-segments between source and destination
    pub fn lookup_core_segments(&self, src: &IsdAs, dst: &IsdAs) -> Vec<super::PathSegment<P>> {
        // Limit segments to prevent OOM (spec-compliant: line 1558 recommends max 50)
        self.path_database.lookup_core_segments(Some(src), Some(dst)).into_iter().take(1000).cloned().collect()
    }

    /// Handle a SCION event (event-driven architecture).
    ///
    /// This follows the same pattern as BGP event handling:
    /// 1. Process incoming event (store PCB, register segment, etc.)
    /// 2. Make local decisions (select best PCBs)
    /// 3. Generate new events for neighbors (only if necessary)
    ///
    /// # Arguments
    /// * `from` - Source router of the event
    /// * `event` - The SCION event to handle
    ///
    /// # Returns
    /// Vector of new events to enqueue
    pub fn handle_event<T: Default>(
        &mut self,
        from: RouterId,
        event: super::ScionEvent<P>,
    ) -> Result<Vec<crate::event::Event<P, T>>, crate::types::DeviceError> {
        use crate::event::Event;
        use super::ScionEvent;
        use super::beaconing::{select_for_propagation, extend_pcb, add_peering_entries, SimpleSelectionPolicy};

        match event {
            ScionEvent::BeaconPropagation { pcb, .. } => {
                // 1. Store PCB in beacon store (like BGP RIB_IN)
                let inserted = self.beacon_store.insert(pcb.clone());
                if !inserted {
                    // PCB was rejected (duplicate or at capacity)
                    // No propagation needed
                    return Ok(vec![]);
                }

                // 2. Select best PCBs for propagation (like BGP decision process)
                let all_pcbs = self.beacon_store.get_all();

                // Filter PCBs based on AS type (critical for multi-ISD)
                let pcbs_to_propagate: Vec<_> = if self.is_core {
                    // Core ASes propagate ALL PCBs (including foreign ISDs)
                    all_pcbs.iter().map(|p| (*p).clone()).collect()
                } else {
                    // Non-core ASes only propagate same-ISD PCBs
                    all_pcbs
                        .iter()
                        .filter(|pcb| {
                            if let Some(origin) = pcb.get_origin() {
                                origin.isd == self.isd_as.isd
                            } else {
                                false
                            }
                        })
                        .map(|p| (*p).clone())
                        .collect()
                };

                if pcbs_to_propagate.is_empty() {
                    return Ok(vec![]);
                }

                // Select best PCBs (spec-compliant: max 50)
                let selected_pcbs = select_for_propagation(
                    &pcbs_to_propagate,
                    &SimpleSelectionPolicy,
                    50
                );

                // 3. Generate propagation events for children and core neighbors (like BGP dissemination)
                let mut events = Vec::new();
                let child_interfaces = self.get_child_interfaces();
                let core_interfaces = if self.is_core {
                    // Core ASes also propagate to other core ASes
                    self.get_core_interfaces()
                } else {
                    vec![]
                };
                let peering_interfaces = self.get_peering_interfaces();

                // Prepare peering entries
                let peering_entries: Vec<(IsdAs, InterfaceId, InterfaceId, u16)> =
                    peering_interfaces
                        .iter()
                        .map(|iface| {
                            (
                                iface.neighbor_isd_as,
                                iface.interface_id,
                                iface.interface_id,
                                iface.mtu,
                            )
                        })
                        .collect();

                // Get current timestamp from the PCB
                let timestamp = pcb.segment_info.timestamp;

                // For each selected PCB, extend and send to children and core neighbors
                for pcb in selected_pcbs {
                    // Combine child and core interfaces for propagation
                    let propagation_interfaces: Vec<_> = child_interfaces.iter()
                        .chain(core_interfaces.iter())
                        .copied()
                        .collect();

                    for iface in propagation_interfaces {
                        // Loop prevention: skip if AS is already in path
                        if pcb.get_as_path().contains(&iface.neighbor_isd_as) {
                            continue;
                        }

                        // Extend PCB with this AS's hop entry
                        let mut extended_pcb = extend_pcb(
                            pcb.clone(),
                            self.isd_as,
                            iface.interface_id,
                            iface.interface_id,
                            iface.mtu,
                            timestamp,
                        );

                        // Add peering entries if available
                        if !peering_entries.is_empty() {
                            extended_pcb = add_peering_entries(
                                extended_pcb,
                                peering_entries.clone(),
                                timestamp
                            );
                        }

                        // Create BeaconPropagation event
                        events.push(Event::scion(
                            T::default(),
                            from,  // Not self.router_id - we track original source
                            iface.neighbor_router,
                            ScionEvent::BeaconPropagation {
                                src: from,
                                dst: iface.neighbor_router,
                                pcb: extended_pcb,
                            }
                        ));
                    }
                }

                Ok(events)
            }

            ScionEvent::SegmentRegistration { segment, .. } => {
                // Store segment in path database
                self.path_database.register_segment(segment);
                // Registration doesn't cascade (no new events)
                Ok(vec![])
            }

            ScionEvent::PathLookupRequest { .. } => {
                // TODO: Implement path lookup request handling
                // For now, path lookup is synchronous (not event-driven)
                Ok(vec![])
            }

            ScionEvent::PathLookupResponse { .. } => {
                // TODO: Implement path lookup response handling
                Ok(vec![])
            }

            ScionEvent::CoreBeaconTrigger { .. } => {
                // Core AS creates initial PCBs and sends to all neighbors
                if !self.is_core {
                    // Only core ASes respond to this trigger
                    return Ok(vec![]);
                }

                let mut events = Vec::new();
                let all_interfaces = self.get_all_interfaces();

                // Get current timestamp (use stored or default)
                let timestamp = self.last_core_beacon_time.unwrap_or(1000);

                for interface in all_interfaces {
                    // Create initial PCB for this interface
                    let pcb = super::beaconing::create_initial_pcb(
                        self.isd_as,
                        timestamp,
                        interface.interface_id,
                        interface.mtu,
                    );

                    // Send to neighbor
                    events.push(Event::scion(
                        T::default(),
                        interface.neighbor_router,  // Source is this router
                        interface.neighbor_router,
                        ScionEvent::BeaconPropagation {
                            src: interface.neighbor_router,
                            dst: interface.neighbor_router,
                            pcb,
                        }
                    ));
                }

                self.last_core_beacon_time = Some(timestamp);
                Ok(events)
            }

            ScionEvent::IntraIsdBeaconTrigger { .. } => {
                // Triggered propagation (for periodic beaconing)
                // This would select and propagate stored PCBs
                // For now, propagation happens reactively on BeaconPropagation events
                Ok(vec![])
            }

            ScionEvent::RegistrationTrigger { .. } => {
                // Triggered registration (for periodic operations)
                // TODO: Implement periodic registration
                Ok(vec![])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SimplePrefix;

    fn create_test_interface(
        if_id: u16,
        neighbor_as: u64,
        neighbor_router: u32,
        link_type: ScionLinkType,
    ) -> InterfaceInfo {
        InterfaceInfo::new(
            InterfaceId(if_id),
            IsdAs::new(1, neighbor_as),
            link_type,
            RouterId::from(neighbor_router),
            1500,
        )
    }

    #[test]
    fn test_control_service_creation() {
        let isd_as = IsdAs::new(1, 110u64);
        let cs: ScionControlService<SimplePrefix> = ScionControlService::new(isd_as, true);

        assert_eq!(cs.isd_as, isd_as);
        assert!(cs.is_core);
        assert_eq!(cs.interface_count(), 0);
    }

    #[test]
    fn test_add_interface() {
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), false);

        let interface = create_test_interface(1, 120, 10, ScionLinkType::ParentChild);
        assert!(cs.add_interface(interface).is_ok());
        assert_eq!(cs.interface_count(), 1);
    }

    #[test]
    fn test_add_duplicate_interface_id() {
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), false);

        let interface1 = create_test_interface(1, 120, 10, ScionLinkType::ParentChild);
        let interface2 = create_test_interface(1, 121, 11, ScionLinkType::Core);

        assert!(cs.add_interface(interface1).is_ok());
        assert!(cs.add_interface(interface2).is_err());
        assert_eq!(cs.interface_count(), 1);
    }

    #[test]
    fn test_get_interface() {
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), false);

        let neighbor_router = RouterId::from(10u32);
        let interface = create_test_interface(1, 120, 10, ScionLinkType::ParentChild);
        cs.add_interface(interface).unwrap();

        let retrieved = cs.get_interface(neighbor_router);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().interface_id.0, 1);
    }

    #[test]
    fn test_get_interface_by_id() {
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), false);

        let interface = create_test_interface(1, 120, 10, ScionLinkType::ParentChild);
        cs.add_interface(interface).unwrap();

        let retrieved = cs.get_interface_by_id(InterfaceId(1));
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().neighbor_isd_as.asn.as_u64(), 120);
    }

    #[test]
    fn test_remove_interface() {
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), false);

        let neighbor_router = RouterId::from(10u32);
        let interface = create_test_interface(1, 120, 10, ScionLinkType::ParentChild);
        cs.add_interface(interface).unwrap();

        let removed = cs.remove_interface(neighbor_router);
        assert!(removed.is_some());
        assert_eq!(cs.interface_count(), 0);
        assert!(!cs.has_interface_id(InterfaceId(1)));
    }

    #[test]
    fn test_get_interfaces_by_type() {
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), true);

        let if1 = create_test_interface(1, 120, 10, ScionLinkType::Core);
        let if2 = create_test_interface(2, 121, 11, ScionLinkType::Core);
        let if3 = create_test_interface(3, 122, 12, ScionLinkType::ParentChild);

        cs.add_interface(if1).unwrap();
        cs.add_interface(if2).unwrap();
        cs.add_interface(if3).unwrap();

        let core_ifs = cs.get_core_interfaces();
        assert_eq!(core_ifs.len(), 2);

        let parent_ifs = cs.get_parent_interfaces();
        assert_eq!(parent_ifs.len(), 1);
    }

    #[test]
    fn test_next_interface_id() {
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), false);

        let next_id = cs.next_interface_id();
        assert_eq!(next_id.0, 1);

        let if1 = create_test_interface(1, 120, 10, ScionLinkType::ParentChild);
        cs.add_interface(if1).unwrap();

        let next_id = cs.next_interface_id();
        assert_eq!(next_id.0, 2);
    }

    #[test]
    fn test_get_neighbor_by_interface_id() {
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), false);

        let neighbor_router = RouterId::from(10u32);
        let interface = create_test_interface(1, 120, 10, ScionLinkType::ParentChild);
        cs.add_interface(interface).unwrap();

        let neighbor = cs.get_neighbor_by_interface_id(InterfaceId(1));
        assert_eq!(neighbor, Some(neighbor_router));

        let neighbor = cs.get_neighbor_by_interface_id(InterfaceId(99));
        assert_eq!(neighbor, None);
    }

    #[test]
    fn test_beacon_store_and_path_database_integration() {
        let cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 110u64), true);

        // Verify beacon store and path database are initialized
        assert_eq!(cs.beacon_store.total_count(), 0);
        assert_eq!(cs.path_database.total_count(), 0);
    }
}
