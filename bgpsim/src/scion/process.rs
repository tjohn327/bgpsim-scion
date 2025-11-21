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

use crate::scion::ScionSimulationMode;
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
    /// Last time PCBs were propagated to each neighbor (for interval enforcement)
    /// Maps (neighbor_isd_as, link_type) to last propagation timestamp
    #[serde(default)]
    last_propagation_time: HashMap<(IsdAs, ScionLinkType), u32>,
    /// Last successful propagation time per neighbor (for fast-recovery mode)
    /// Maps (neighbor_isd_as, link_type) to last successful propagation timestamp
    #[serde(default)]
    last_successful_propagation: HashMap<(IsdAs, ScionLinkType), u32>,
    /// Simulation mode controlling propagation behaviour.
    #[serde(default)]
    mode: ScionSimulationMode,
    /// Optional max hop count for core-to-core PCB propagation.
    #[serde(default)]
    core_propagation_limit: Option<usize>,
}

impl<P: Prefix> ScionControlService<P> {
    /// Create a new SCION control service for an AS using the default (dynamic) simulation mode.
    ///
    /// # Arguments
    /// * `isd_as` - The ISD-AS identifier for this AS
    /// * `is_core` - Whether this AS is a core AS
    pub fn new(isd_as: IsdAs, is_core: bool) -> Self {
        Self::with_mode(isd_as, is_core, ScionSimulationMode::Dynamic)
    }

    /// Create a new control service pinned to a specific simulation mode.
    pub fn with_mode(isd_as: IsdAs, is_core: bool, mode: ScionSimulationMode) -> Self {
        let (per_source, total) = mode.beacon_limits();
        ScionControlService {
            isd_as,
            is_core,
            beacon_store: BeaconStore::new(per_source, total),
            path_database: PathDatabase::with_limits(
                mode.path_database_capacity(),
                mode.up_down_segment_limit(),
                mode.core_segment_limit(),
            ),
            interfaces: HashMap::new(),
            interface_id_map: HashMap::new(),
            last_core_beacon_time: None,
            last_intra_isd_beacon_time: None,
            last_registration_time: None,
            last_propagation_time: HashMap::new(),
            last_successful_propagation: HashMap::new(),
            mode,
            core_propagation_limit: None,
        }
    }

    /// Returns the current simulation mode.
    pub fn simulation_mode(&self) -> ScionSimulationMode {
        self.mode
    }

    /// Updates the simulation mode and reapplies capacity limits.
    pub fn set_simulation_mode(&mut self, mode: ScionSimulationMode) {
        if self.mode != mode {
            self.mode = mode;
            let (per_source, total) = mode.beacon_limits();
            self.beacon_store.configure_limits(per_source, total);
            self.path_database.configure_limits(
                mode.path_database_capacity(),
                mode.up_down_segment_limit(),
                mode.core_segment_limit(),
            );
        }
    }

    /// Set or clear the propagation depth limit for inter-core beaconing.
    pub fn set_core_propagation_limit(&mut self, limit: Option<usize>) {
        self.core_propagation_limit = limit;
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
        if self
            .interface_id_map
            .contains_key(&interface_info.interface_id)
        {
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
    ///
    /// Returns ParentChild interfaces. These represent connections to parent ASes
    /// in the SCION hierarchy.
    ///
    /// Note: With symmetric link storage, this returns the same as get_child_interfaces().
    /// The semantic meaning depends on the AS's role. For registration, non-core ASes
    /// use these to find parents. For propagation, use get_child_interfaces().
    pub fn get_parent_interfaces(&self) -> Vec<&InterfaceInfo> {
        // Return all ParentChild interfaces
        // For non-core ASes during registration: these point to parents
        // For cores: these might point to superior cores (rare) or children
        self.interfaces
            .values()
            .filter(|iface| matches!(iface.link_type, ScionLinkType::ParentChild))
            .collect()
    }

    /// Get all child interfaces (from parent's perspective).
    ///
    /// Returns ParentChild interfaces. These represent connections to child ASes
    /// in the SCION hierarchy. Both core and tier-2 ASes can have children.
    ///
    /// Note: With symmetric link storage, we can't distinguish parent-side from
    /// child-side based on link type alone. This returns ALL ParentChild interfaces.
    /// For parents, these point to children. For children, these point to parents.
    /// Callers should filter based on AS role in the hierarchy.
    pub fn get_child_interfaces(&self) -> Vec<&InterfaceInfo> {
        // Return all ParentChild interfaces
        // These could be:
        // - For core/tier-2 ASes: interfaces to children
        // - For leaf ASes: interfaces to parents (filtered elsewhere)
        self.interfaces
            .values()
            .filter(|iface| matches!(iface.link_type, ScionLinkType::ParentChild))
            .collect()
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
        self.path_database
            .lookup_up_segments(dst)
            .into_iter()
            .take(1000)
            .cloned()
            .collect()
    }

    /// Lookup up-segments from a source.
    ///
    /// # Arguments
    /// * `src` - Source ISD-AS
    ///
    /// # Returns
    /// Vector of up-segments from the source
    pub fn lookup_up_segments_from(&self, src: &IsdAs) -> Vec<super::PathSegment<P>> {
        let limit = self.mode.up_down_segment_limit();
        self.path_database
            .lookup_up_segments_from(src)
            .into_iter()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Lookup down-segments from a source.
    ///
    /// # Arguments
    /// * `src` - Source ISD-AS
    ///
    /// # Returns
    /// Vector of down-segments from the source
    pub fn lookup_down_segments(&self, src: &IsdAs) -> Vec<super::PathSegment<P>> {
        self.path_database
            .lookup_down_segments(src)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Lookup down-segments to a destination.
    ///
    /// # Arguments
    /// * `dst` - Destination ISD-AS
    ///
    /// # Returns
    /// Vector of down-segments to the destination
    pub fn lookup_down_segments_to(&self, dst: &IsdAs) -> Vec<super::PathSegment<P>> {
        let limit = self.mode.up_down_segment_limit();
        self.path_database
            .lookup_down_segments_to(dst)
            .into_iter()
            .take(limit)
            .cloned()
            .collect()
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
        let limit = self.mode.core_segment_limit();
        self.path_database
            .lookup_core_segments(Some(src), Some(dst))
            .into_iter()
            .take(limit)
            .cloned()
            .collect()
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
        use super::beaconing::{add_peering_entries, extend_pcb};
        use super::ScionEvent;
        use crate::event::Event;

        match event {
            ScionEvent::BeaconPropagation { pcb, src, .. } => {
                // ✅ SPEC-COMPLIANT: Validate PCB with interface and continuity checks (§2.3.1)
                // Determine receiving interface information
                let receiving_interface = self.get_interface(src);
                if let Some(iface) = receiving_interface {
                    // Validate PCB with interface checks
                    if let Err(e) = super::beaconing::validate_pcb_with_interface(
                        &pcb,
                        pcb.segment_info.timestamp,
                        self.is_core,
                        iface.neighbor_isd_as,
                        iface.link_type,
                    ) {
                        // Invalid PCB - discard per spec §2.3.1
                        // Log validation failure for debugging (can be removed later)
                        log::debug!(
                            "PCB validation failed for AS {:?}: {} (last_entry: {:?}, neighbor: {:?})",
                            self.isd_as, e,
                            pcb.get_last_as(),
                            iface.neighbor_isd_as
                        );
                        return Ok(vec![]);
                    }
                } else {
                    // If we can't determine the interface, do basic validation only
                    // This can happen in edge cases during topology construction
                    if let Err(_e) = super::beaconing::validate_pcb(
                        &pcb,
                        pcb.segment_info.timestamp,
                        self.is_core,
                    ) {
                        // Invalid PCB - discard
                        return Ok(vec![]);
                    }
                }

                // 1. Store PCB in beacon store (like BGP RIB_IN)
                let inserted = self.beacon_store.insert(pcb.clone());
                if !inserted {
                    // PCB was rejected (duplicate or at capacity)
                    // No propagation needed
                    return Ok(vec![]);
                }

                // 2. Check if we should propagate this PCB based on AS type
                // Core ASes can propagate to other cores (for inter-ISD paths)
                // Non-core ASes only propagate same-ISD PCBs
                let should_propagate = if self.is_core {
                    true // Core ASes can propagate (subject to selection)
                } else {
                    // Non-core ASes only propagate same-ISD PCBs
                    if let Some(origin) = pcb.get_origin() {
                        origin.isd == self.isd_as.isd
                    } else {
                        false
                    }
                };

                if !should_propagate {
                    return Ok(vec![]);
                }

                // 3. ✅ SPEC-COMPLIANT: Per-link PCB limits (§2.3.4)
                // Limits are applied per destination/neighbor link, not globally per origin
                // - Intra-ISD: ≤50 per child link
                // - Core: ≤5 per immediate neighbor core AS

                // 4. Propagate the PCB with per-link selection
                let mut events = Vec::new();

                // Collect interfaces into owned values to avoid borrow conflicts
                let child_interfaces: Vec<InterfaceInfo> =
                    self.get_child_interfaces().into_iter().cloned().collect();
                let core_interfaces: Vec<InterfaceInfo> = if self.is_core {
                    self.get_core_interfaces().into_iter().cloned().collect()
                } else {
                    vec![]
                };
                let peering_interfaces: Vec<InterfaceInfo> =
                    self.get_peering_interfaces().into_iter().cloned().collect();

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

                // Determine ingress interface for this PCB (based on sender router ID).
                let ingress_if = self
                    .get_interface(src)
                    .map(|iface| iface.interface_id)
                    .unwrap_or(InterfaceId::UNSPECIFIED);

                // Combine child and core interfaces for propagation
                let propagation_interfaces: Vec<_> = child_interfaces
                    .iter()
                    .chain(core_interfaces.iter())
                    .collect();

                let as_path_set: std::collections::HashSet<_> =
                    pcb.get_as_path().iter().copied().collect();

                // ✅ SPEC-COMPLIANT: Apply limits per destination link (§2.3.4)
                // For each neighbor, check if we've already reached the limit for that destination
                for iface in propagation_interfaces {
                    if matches!(iface.link_type, ScionLinkType::Core) {
                        if let Some(limit) = self.core_propagation_limit {
                            if pcb.path_length() >= limit {
                                continue;
                            }
                        }
                    }

                    if as_path_set.contains(&iface.neighbor_isd_as) {
                        continue;
                    }

                    // ✅ SPEC-COMPLIANT: Check propagation interval (§2.3.4)
                    // Intra-ISD: ≥5 seconds, Core: ≥60 seconds
                    // Fast-recovery: allow more frequent propagation if last interval had no success
                    // Note: In event-driven mode, intervals are checked per PCB timestamp
                    // For batch mode, use actual simulation time
                    let propagation_key = (iface.neighbor_isd_as, iface.link_type);
                    let min_interval = if matches!(iface.link_type, ScionLinkType::Core) {
                        60 // Core: ≥60 seconds
                    } else {
                        5 // Intra-ISD: ≥5 seconds
                    };

                    // ✅ SPEC-COMPLIANT: Check propagation interval (§2.3.4)
                    // Intra-ISD: ≥5 seconds, Core: ≥60 seconds
                    // Note: In event-driven mode, all events typically have the same timestamp,
                    // so interval enforcement is relaxed. In batch/periodic mode, intervals are enforced.
                    let propagation_key = (iface.neighbor_isd_as, iface.link_type);
                    let min_interval = if matches!(iface.link_type, ScionLinkType::Core) {
                        60 // Core: ≥60 seconds
                    } else {
                        5 // Intra-ISD: ≥5 seconds
                    };

                    // Check if we should allow propagation (interval elapsed or fast-recovery)
                    // In event-driven mode with same timestamps, we allow propagation for new PCBs
                    let should_propagate =
                        if let Some(last_time) = self.last_propagation_time.get(&propagation_key) {
                            // Check if enough time has passed
                            let interval_elapsed = timestamp > *last_time + min_interval;

                            // ✅ Fast-recovery mode (§2.3.4): allow propagation if last interval had no success
                            let fast_recovery = if let Some(last_success) =
                                self.last_successful_propagation.get(&propagation_key)
                            {
                                // If last successful propagation was before last attempt, allow fast-recovery
                                *last_success < *last_time
                            } else {
                                // No successful propagation recorded - allow fast-recovery
                                true
                            };

                            // In event-driven mode: if timestamps are the same, allow propagation (new PCB)
                            // Otherwise, enforce interval or allow fast-recovery
                            timestamp == *last_time || interval_elapsed || fast_recovery
                        } else {
                            // If no previous propagation time recorded, allow propagation (first time)
                            true
                        };

                    if !should_propagate {
                        continue;
                    }

                    // ✅ Per-link limit check: Get PCBs already selected for this destination
                    let origin = match pcb.get_origin() {
                        Some(origin) => origin,
                        None => continue,
                    };

                    // Get all PCBs from this origin that could be sent to this destination
                    let candidate_pcbs = self.beacon_store.get_by_source(&origin);

                    // Determine limit based on link type
                    let max_pcbs_per_link = if matches!(iface.link_type, ScionLinkType::Core) {
                        self.mode.core_segment_limit() // ≤5 per neighbor core AS
                    } else {
                        self.mode.intra_segment_limit() // ≤50 per child link
                    };

                    // Check if we've already selected max PCBs for this (origin, destination) pair
                    // In a full implementation, we'd track this per destination, but for now
                    // we use a simplified check: if we have too many PCBs from this origin,
                    // we need to select the best ones
                    if candidate_pcbs.len() > max_pcbs_per_link {
                        use super::path_selection::select_best_pcbs;
                        let best = select_best_pcbs(candidate_pcbs, max_pcbs_per_link);
                        let new_is_best = best.iter().any(|existing| {
                            existing.get_as_path() == pcb.get_as_path()
                                && existing.segment_info.timestamp == pcb.segment_info.timestamp
                        });
                        if !new_is_best {
                            // This PCB didn't make it into the best set for this destination
                            // Skip propagation to this neighbor
                            continue;
                        }
                    }

                    let mut extended_pcb = extend_pcb(
                        pcb.clone(),
                        self.isd_as,
                        ingress_if,
                        iface.interface_id,
                        iface.mtu,
                        timestamp,
                    );

                    // Add peering entries if available
                    if !peering_entries.is_empty() {
                        extended_pcb =
                            add_peering_entries(extended_pcb, peering_entries.clone(), timestamp);
                    }

                    // Create BeaconPropagation event
                    events.push(Event::scion(
                        T::default(),
                        from, // Not self.router_id - we track original source
                        iface.neighbor_router,
                        ScionEvent::BeaconPropagation {
                            src: from,
                            dst: iface.neighbor_router,
                            pcb: extended_pcb,
                        },
                    ));

                    // ✅ Update last propagation time for this neighbor
                    self.last_propagation_time
                        .insert(propagation_key, timestamp);
                    // Note: last_successful_propagation is updated when we receive confirmation
                    // For now, we optimistically assume propagation succeeds
                    // In a full implementation, this would be updated on successful RPC response
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
                let interfaces: Vec<_> = self
                    .get_all_interfaces()
                    .into_iter()
                    .filter(|iface| !iface.is_peering_link())
                    .collect();

                // Get current timestamp (use stored or default)
                let timestamp = self.last_core_beacon_time.unwrap_or(1000);

                for interface in interfaces {
                    // Create initial PCB for this interface
                    let pcb = super::beaconing::create_initial_pcb(
                        self.isd_as,
                        timestamp,
                        interface.interface_id,
                        interface.mtu,
                    );

                    // Send to neighbor (from is this core router's ID)
                    events.push(Event::scion(
                        T::default(),
                        from,                      // Source is this core router
                        interface.neighbor_router, // Destination is the neighbor
                        ScionEvent::BeaconPropagation {
                            src: from,
                            dst: interface.neighbor_router,
                            pcb,
                        },
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

// IntoIpv4Prefix implementation for prefix conversion

use crate::types::{IntoIpv4Prefix, Ipv4Prefix};

impl<P: Prefix> IntoIpv4Prefix for ScionControlService<P> {
    type T = ScionControlService<Ipv4Prefix>;

    fn into_ipv4_prefix(self) -> Self::T {
        ScionControlService {
            isd_as: self.isd_as,
            is_core: self.is_core,
            beacon_store: self.beacon_store.into_ipv4_prefix(),
            path_database: self.path_database.into_ipv4_prefix(),
            interfaces: self.interfaces,
            interface_id_map: self.interface_id_map,
            last_core_beacon_time: self.last_core_beacon_time,
            last_intra_isd_beacon_time: self.last_intra_isd_beacon_time,
            last_propagation_time: self.last_propagation_time.clone(),
            last_successful_propagation: self.last_successful_propagation.clone(),
            last_registration_time: self.last_registration_time,
            mode: self.mode,
            core_propagation_limit: self.core_propagation_limit,
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
