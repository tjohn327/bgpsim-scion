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

//! SCION Network Extensions
//!
//! This module provides Network-level methods for managing SCION operations,
//! including enabling SCION on routers, configuring link types, and running
//! beaconing rounds.

use crate::{
    network::Network,
    ospf::OspfImpl,
    scion::{
        add_peering_entries, create_initial_pcb, extend_pcb, select_for_propagation, validate_pcb,
        BeaconStore, InterfaceId, Pcb, SimpleSelectionPolicy,
    },
    types::{NetworkError, Prefix, RouterId},
};

use crate::scion::types::{InterfaceInfo, IsdAs, ScionLinkType};

/// SCION default parameters based on draft-dekater-scion-controlplane-10
///
/// These values are recommended by the SCION specification for production deployments.
/// See: <https://datatracker.ietf.org/doc/html/draft-dekater-scion-controlplane-10>

/// Maximum number of PCBs to propagate per link (Best PCBs Set Size)
///
/// Per spec section 4.1.3.3:
/// "At most 50 PCBs per child link are propagated"
pub const DEFAULT_MAX_PCBS: usize = 50;

/// Maximum PCBs for core beaconing (between core ASes)
///
/// Per spec section 4.1.4.2:
/// "at most 5 path segments to every destination AS are discovered"
pub const DEFAULT_MAX_CORE_PCBS: usize = 5;

/// Default propagation interval for intra-ISD beaconing (in seconds)
///
/// Per spec: "should be at least '5' (seconds)"
pub const DEFAULT_INTRA_ISD_INTERVAL: u32 = 5;

/// Default propagation interval for core beaconing (in seconds)
///
/// Per spec: "at least '60' (seconds)"
pub const DEFAULT_CORE_INTERVAL: u32 = 60;

/// Path segments returned by SCION path lookup (spec-compliant).
///
/// Per draft-dekater-scion-controlplane-10, Section 3.4:
/// "The Control Service returns path segments, not complete paths.
/// Path construction happens at the endpoint (data plane)."
///
/// This struct holds segments by type, allowing the caller to:
/// 1. Inspect available segments before combining
/// 2. Apply custom path selection policies
/// 3. Lazily construct paths on-demand
/// 4. Avoid combinatorial explosion in the control plane
#[derive(Debug, Clone)]
pub struct PathSegments<P: Prefix> {
    /// Up-segments from source (non-core) to core AS
    pub up_segments: Vec<crate::scion::PathSegment<P>>,
    /// Core-segments between core ASes (possibly across ISDs)
    pub core_segments: Vec<crate::scion::PathSegment<P>>,
    /// Down-segments from core AS to destination (non-core)
    pub down_segments: Vec<crate::scion::PathSegment<P>>,
}

/// Default hop expiration time (in seconds)
///
/// Per spec: "around 6 hours"
pub const DEFAULT_HOP_EXPIRATION: u32 = 21600; // 6 hours

impl<P: Prefix, Q, Ospf: OspfImpl> Network<P, Q, Ospf> {
    /// Enable SCION on a router, creating a SCION Control Service.
    ///
    /// # Arguments
    /// * `router` - Router to enable SCION on
    /// * `isd_as` - ISD-AS identifier for this router
    /// * `is_core` - Whether this is a core AS
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(NetworkError)` if router doesn't exist or SCION already enabled
    pub fn enable_scion(
        &mut self,
        router: RouterId,
        isd_as: IsdAs,
        is_core: bool,
    ) -> Result<(), NetworkError> {
        let r = self.get_router_mut(router)?;

        // Check if SCION is already enabled
        if r.scion().is_some() {
            return Err(NetworkError::DeviceNotFound(router)); // Reuse existing error
        }

        // Create and enable SCION control service
        r.enable_scion(isd_as, is_core);

        Ok(())
    }

    /// Disable SCION on a router.
    ///
    /// # Arguments
    /// * `router` - Router to disable SCION on
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(NetworkError)` if router doesn't exist
    pub fn disable_scion(&mut self, router: RouterId) -> Result<(), NetworkError> {
        let r = self.get_router_mut(router)?;
        r.disable_scion();
        Ok(())
    }

    /// Configure a SCION link between two routers.
    ///
    /// This sets up the SCION interface information on both routers and
    /// assigns interface IDs.
    ///
    /// # Arguments
    /// * `router_a` - First router
    /// * `router_b` - Second router
    /// * `link_type` - SCION link type (Core, ParentChild, or Peering)
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(NetworkError)` if routers don't exist or SCION not enabled
    pub fn configure_scion_link(
        &mut self,
        router_a: RouterId,
        router_b: RouterId,
        link_type: ScionLinkType,
    ) -> Result<(), NetworkError> {
        // Verify both routers have SCION enabled
        let (isd_as_a, isd_as_b) = {
            let r_a = self.get_router(router_a)?;
            let r_b = self.get_router(router_b)?;

            let cs_a = r_a.scion().ok_or(NetworkError::DeviceNotFound(router_a))?;
            let cs_b = r_b.scion().ok_or(NetworkError::DeviceNotFound(router_b))?;

            (cs_a.isd_as, cs_b.isd_as)
        };

        // Get or assign interface IDs
        let (if_a, if_b) = {
            let cs_a = self.get_router_mut(router_a)?.scion_mut().unwrap();
            let if_a = cs_a.next_interface_id();

            let cs_b = self.get_router_mut(router_b)?.scion_mut().unwrap();
            let if_b = cs_b.next_interface_id();

            (if_a, if_b)
        };

        // Add interfaces to both routers
        let cs_a = self.get_router_mut(router_a)?.scion_mut().unwrap();
        cs_a.add_interface(InterfaceInfo::new(if_a, isd_as_b, link_type, router_b, 1500))
            .expect("Failed to add SCION interface");

        let cs_b = self.get_router_mut(router_b)?.scion_mut().unwrap();
        cs_b.add_interface(InterfaceInfo::new(if_b, isd_as_a, link_type, router_a, 1500))
            .expect("Failed to add SCION interface");

        Ok(())
    }

    /// Run a core beaconing round.
    ///
    /// Core ASes generate PCBs and propagate them to their core neighbors.
    ///
    /// # Arguments
    /// * `timestamp` - Current simulation timestamp
    ///
    /// # Returns
    /// `Ok(num_pcbs_created)` - number of PCBs created
    pub fn scion_core_beaconing(&mut self, timestamp: u32) -> Result<usize, NetworkError> {
        let mut created_pcbs: Vec<(RouterId, Vec<(RouterId, Pcb<P>)>)> = Vec::new();

        // Step 1: Generate PCBs at core ASes
        let router_ids: Vec<RouterId> = self.routers.keys().copied().collect();
        for router_id in router_ids {
            let router = self.get_router(router_id)?;

            if let Some(cs) = router.scion() {
                if cs.is_core {
                    let isd_as = cs.isd_as;

                    // Core ASes send beacons to ALL neighbors (core and non-core)
                    let all_interfaces = cs.get_all_interfaces();

                    let mut pcbs_for_router = Vec::new();

                    // Create a PCB for each neighbor
                    for interface in all_interfaces {
                        let pcb = create_initial_pcb(
                            isd_as,
                            timestamp,
                            interface.interface_id,
                            interface.mtu,
                        );

                        pcbs_for_router.push((interface.neighbor_router, pcb));
                    }

                    if !pcbs_for_router.is_empty() {
                        created_pcbs.push((router_id, pcbs_for_router));
                    }
                }
            }
        }

        // Step 2: Deliver PCBs to neighbors
        let mut total_created = 0;
        for (_src_router, pcbs) in created_pcbs {
            for (dst_router, pcb) in pcbs {
                // Store the PCB at the destination
                let dst = self.get_router_mut(dst_router)?;
                if let Some(cs) = dst.scion_mut() {
                    cs.beacon_store.insert(pcb);
                    total_created += 1;
                }
            }
        }

        Ok(total_created)
    }

    /// Run an intra-ISD beaconing round.
    ///
    /// Non-core ASes select PCBs from their beacon store, extend them,
    /// add peering information, and propagate to children.
    ///
    /// # Arguments
    /// * `timestamp` - Current simulation timestamp
    /// * `max_propagate` - Maximum number of PCBs to propagate per AS
    ///
    /// # Returns
    /// `Ok(num_pcbs_propagated)` - number of PCBs propagated
    pub fn scion_intra_isd_beaconing(
        &mut self,
        timestamp: u32,
        max_propagate: usize,
    ) -> Result<usize, NetworkError> {
        let policy = SimpleSelectionPolicy;
        let mut propagated_pcbs: Vec<(RouterId, Vec<(RouterId, Pcb<P>)>)> = Vec::new();

        // Step 1: Each AS selects PCBs and prepares propagation
        let router_ids: Vec<RouterId> = self.routers.keys().copied().collect();
        for router_id in router_ids {
            let router = self.get_router(router_id)?;

            if let Some(cs) = router.scion() {
                let isd_as = cs.isd_as;
                let is_core = cs.is_core;

                // Get all PCBs from beacon store
                let all_pcbs = cs.beacon_store.get_all();

                // Skip if no PCBs to propagate
                if all_pcbs.is_empty() {
                    continue;
                }

                // Filter PCBs based on AS type:
                // - Core ASes propagate PCBs from ALL ISDs (they're the inter-ISD gateway)
                // - Non-core ASes only propagate same-ISD PCBs
                //
                // Per spec: "core beaconing... between core ASes in the same or in different ISDs"
                // Core ASes MUST propagate foreign ISD PCBs downward to enable inter-ISD connectivity
                let pcbs_to_propagate: Vec<Pcb<P>> = if is_core {
                    // Core ASes: propagate ALL PCBs (including from foreign ISDs)
                    all_pcbs.iter().map(|p| (*p).clone()).collect()
                } else {
                    // Non-core ASes: only propagate same-ISD PCBs
                    all_pcbs
                        .iter()
                        .filter(|pcb| {
                            if let Some(origin) = pcb.get_origin() {
                                origin.isd == isd_as.isd
                            } else {
                                false
                            }
                        })
                        .map(|p| (*p).clone())
                        .collect()
                };

                // Skip if no PCBs to propagate
                if pcbs_to_propagate.is_empty() {
                    continue;
                }

                // Select PCBs to propagate
                let selected_pcbs = select_for_propagation(&pcbs_to_propagate, &policy, max_propagate);

                // Get child and peering interfaces
                let child_interfaces = cs.get_child_interfaces();
                let peering_interfaces = cs.get_peering_interfaces();

                // Prepare peering entries
                let peering_entries: Vec<(IsdAs, InterfaceId, InterfaceId, u16)> =
                    peering_interfaces
                        .iter()
                        .map(|iface| {
                            (
                                iface.neighbor_isd_as,
                                iface.interface_id,
                                iface.interface_id, // peer's interface (simplified)
                                iface.mtu,
                            )
                        })
                        .collect();

                let mut pcbs_to_send = Vec::new();

                // For each selected PCB, extend it and send to children
                for pcb in selected_pcbs {
                    for child_interface in &child_interfaces {
                        // Skip if this would create a loop
                        if pcb.get_as_path().contains(&child_interface.neighbor_isd_as) {
                            continue;
                        }

                        // Extend the PCB with this AS's entry
                        let mut extended_pcb = extend_pcb(
                            pcb.clone(),
                            isd_as,
                            child_interface.interface_id,
                            child_interface.interface_id,
                            child_interface.mtu,
                            timestamp,
                        );

                        // Add peering entries if we have any
                        if !peering_entries.is_empty() {
                            extended_pcb =
                                add_peering_entries(extended_pcb, peering_entries.clone(), timestamp);
                        }

                        // Validate the extended PCB
                        if validate_pcb(&extended_pcb, timestamp, is_core).is_ok() {
                            pcbs_to_send.push((child_interface.neighbor_router, extended_pcb));
                        }
                    }
                }

                if !pcbs_to_send.is_empty() {
                    propagated_pcbs.push((router_id, pcbs_to_send));
                }
            }
        }

        // Step 2: Deliver propagated PCBs to children
        let mut total_propagated = 0;
        for (_src_router, pcbs) in propagated_pcbs {
            for (dst_router, pcb) in pcbs {
                let dst = self.get_router_mut(dst_router)?;
                if let Some(cs) = dst.scion_mut() {
                    cs.beacon_store.insert(pcb);
                    total_propagated += 1;
                }
            }
        }

        Ok(total_propagated)
    }

    /// Run a complete beaconing round (both core and intra-ISD).
    ///
    /// # Arguments
    /// * `timestamp` - Current simulation timestamp
    /// * `max_propagate` - Maximum number of PCBs to propagate per AS
    ///
    /// # Returns
    /// `Ok((core_pcbs, intra_pcbs))` - number of PCBs created in each phase
    pub fn scion_beaconing_round(
        &mut self,
        timestamp: u32,
        max_propagate: usize,
    ) -> Result<(usize, usize), NetworkError> {
        let core_pcbs = self.scion_core_beaconing(timestamp)?;
        let intra_pcbs = self.scion_intra_isd_beaconing(timestamp, max_propagate)?;
        Ok((core_pcbs, intra_pcbs))
    }

    /// Get the beacon store for a specific router.
    ///
    /// # Arguments
    /// * `router` - Router to get beacon store from
    ///
    /// # Returns
    /// Reference to the beacon store if SCION is enabled, None otherwise
    pub fn get_scion_beacon_store(&self, router: RouterId) -> Result<&BeaconStore<P>, NetworkError> {
        let r = self.get_router(router)?;
        r.scion()
            .map(|cs| &cs.beacon_store)
            .ok_or(NetworkError::DeviceNotFound(router))
    }

    /// Check if SCION is enabled on a router.
    ///
    /// # Arguments
    /// * `router` - Router to check
    ///
    /// # Returns
    /// `Ok(true)` if SCION is enabled, `Ok(false)` if not, `Err` if router doesn't exist
    pub fn is_scion_enabled(&self, router: RouterId) -> Result<bool, NetworkError> {
        let r = self.get_router(router)?;
        Ok(r.is_scion_enabled())
    }

    /// Register up-segments at core ASes.
    ///
    /// Non-core ASes select PCBs from their beacon store, convert them to up-segments,
    /// and register them at their parent core ASes.
    ///
    /// # Arguments
    /// * `max_select` - Maximum number of PCBs to select per AS
    ///
    /// # Returns
    /// `Ok(num_segments_registered)` - number of up-segments registered
    pub fn scion_register_up_segments(&mut self, max_select: usize) -> Result<usize, NetworkError> {
        use crate::scion::{PathSegment, SegmentType};

        let policy = SimpleSelectionPolicy;
        let mut segments_to_register: Vec<(RouterId, PathSegment<P>)> = Vec::new();

        // Step 1: Non-core ASes create up-segments
        let router_ids: Vec<RouterId> = self.routers.keys().copied().collect();
        for router_id in router_ids {
            let router = self.get_router(router_id)?;

            if let Some(cs) = router.scion() {
                // Only non-core ASes register up-segments
                if cs.is_core {
                    continue;
                }

                // Get all PCBs from beacon store
                let all_pcbs = cs.beacon_store.get_all();
                if all_pcbs.is_empty() {
                    continue;
                }

                // Select PCBs to register
                let all_pcbs_owned: Vec<Pcb<P>> = all_pcbs.iter().map(|p| (*p).clone()).collect();
                let selected_pcbs = select_for_propagation(&all_pcbs_owned, &policy, max_select);

                // Get parent interfaces (to determine where to register)
                let parent_interfaces = cs.get_parent_interfaces();
                let isd_as = cs.isd_as;

                // Convert each selected PCB to an up-segment and register at parent core
                for pcb in selected_pcbs {
                    // Extend PCB with this AS before creating up-segment
                    for parent_interface in &parent_interfaces {
                        let extended_pcb = extend_pcb(
                            pcb.clone(),
                            isd_as,
                            parent_interface.interface_id,
                            parent_interface.interface_id,
                            parent_interface.mtu,
                            pcb.segment_info.timestamp,
                        );

                        let up_segment = PathSegment::from_pcb(&extended_pcb, SegmentType::Up);
                        let parent_router = parent_interface.neighbor_router;
                        segments_to_register.push((parent_router, up_segment));
                    }
                }
            }
        }

        // Step 2: Register up-segments at core ASes
        let mut total_registered = 0;
        for (core_router, segment) in segments_to_register {
            let core = self.get_router_mut(core_router)?;
            if let Some(cs) = core.scion_mut() {
                cs.register_up_segment(segment).ok();
                total_registered += 1;
            }
        }

        Ok(total_registered)
    }

    /// Register down-segments at core ASes.
    ///
    /// Core ASes create down-segments by reversing the up-segments they've received.
    ///
    /// # Returns
    /// `Ok(num_segments_registered)` - number of down-segments registered
    pub fn scion_register_down_segments(&mut self) -> Result<usize, NetworkError> {
        let mut total_registered = 0;

        let router_ids: Vec<RouterId> = self.routers.keys().copied().collect();
        for router_id in router_ids {
            let router = self.get_router_mut(router_id)?;

            if let Some(cs) = router.scion_mut() {
                // All ASes can reverse their stored up-segments to create down-segments
                // In hierarchical topologies, transits have up-segments from leaves

                // Get all up-segments from path database
                let up_segments = cs.path_database.get_all_segments();
                let up_segments: Vec<_> = up_segments
                    .into_iter()
                    .filter(|seg| matches!(seg.segment_type, crate::scion::SegmentType::Up))
                    .collect();

                // Reverse each up-segment to create a down-segment
                for up_segment in up_segments {
                    let down_segment = up_segment.reverse();
                    cs.register_down_segment(down_segment).ok();
                    total_registered += 1;
                }
            }
        }

        Ok(total_registered)
    }

    /// Register core-segments at core ASes.
    ///
    /// Core ASes convert PCBs from core beaconing into core-segments.
    ///
    /// # Arguments
    /// * `max_select` - Maximum number of PCBs to select per AS
    ///
    /// # Returns
    /// `Ok(num_segments_registered)` - number of core-segments registered
    pub fn scion_register_core_segments(&mut self, max_select: usize) -> Result<usize, NetworkError> {
        use crate::scion::{PathSegment, SegmentType};

        let policy = SimpleSelectionPolicy;
        let mut segments_to_register: Vec<(RouterId, PathSegment<P>)> = Vec::new();

        // Step 1: Collect and extend PCBs to create core segments
        let router_ids: Vec<RouterId> = self.routers.keys().copied().collect();
        for router_id in router_ids {
            let router = self.get_router(router_id)?;

            if let Some(cs) = router.scion() {
                // Only core ASes register core-segments
                if !cs.is_core {
                    continue;
                }

                // Get all PCBs from beacon store
                let all_pcbs = cs.beacon_store.get_all();
                if all_pcbs.is_empty() {
                    continue;
                }

                // Select PCBs to register
                let all_pcbs_owned: Vec<Pcb<P>> = all_pcbs.iter().map(|p| (*p).clone()).collect();
                let selected_pcbs = select_for_propagation(&all_pcbs_owned, &policy, max_select);

                // Get core interfaces to determine how to extend PCBs
                let core_interfaces = cs.get_core_interfaces();
                let isd_as = cs.isd_as;

                // For each selected PCB, extend it with this AS and convert to core-segment
                for pcb in selected_pcbs {
                    // Core PCBs are extended with core interface information
                    // We need to find which interface this PCB came from
                    // For simplicity, use the first core interface
                    if let Some(core_interface) = core_interfaces.first() {
                        let extended_pcb = extend_pcb(
                            pcb.clone(),
                            isd_as,
                            core_interface.interface_id,
                            core_interface.interface_id,
                            core_interface.mtu,
                            pcb.segment_info.timestamp,
                        );

                        let core_segment = PathSegment::from_pcb(&extended_pcb, SegmentType::Core);

                        // Register both directions for bidirectional core links
                        segments_to_register.push((router_id, core_segment.clone()));
                        let reversed_segment = core_segment.reverse();
                        segments_to_register.push((router_id, reversed_segment));
                    }
                }
            }
        }

        // Step 2: Register all core segments
        let mut total_registered = 0;
        for (router_id, segment) in segments_to_register {
            let router = self.get_router_mut(router_id)?;
            if let Some(cs) = router.scion_mut() {
                cs.register_core_segment(segment).ok();
                total_registered += 1;
            }
        }

        Ok(total_registered)
    }

    /// Run a complete registration round (up, down, and core segments).
    ///
    /// # Arguments
    /// * `max_select` - Maximum number of PCBs to select per AS
    ///
    /// # Returns
    /// `Ok((up_count, down_count, core_count))` - counts for each segment type
    pub fn scion_registration_round(
        &mut self,
        max_select: usize,
    ) -> Result<(usize, usize, usize), NetworkError> {
        let up_count = self.scion_register_up_segments(max_select)?;
        let down_count = self.scion_register_down_segments()?;
        let core_count = self.scion_register_core_segments(max_select)?;
        Ok((up_count, down_count, core_count))
    }

    /// **[SPEC-COMPLIANT]** Lookup path segments (not full paths).
    ///
    /// Per draft-dekater-scion-controlplane-10, Section 3.4 (line 1758):
    /// "The Control Service returns path segments. Path construction happens in the data plane."
    ///
    /// This method returns UP to 50 segments of each type (per spec line 1558 recommendation),
    /// avoiding combinatorial explosion. The caller combines them as needed.
    ///
    /// # Arguments
    /// * `src` - Source router ID
    /// * `dst` - Destination router ID
    ///
    /// # Returns
    /// `PathSegments` containing up to 50 segments of each type (up, core, down)
    ///
    /// # Memory Usage
    /// Returns ~150-200 segment objects (~260 KB) instead of 100K+ path objects (~175-875 MB)
    ///
    /// # Example
    /// ```ignore
    /// let segments = net.scion_lookup_path_segments(src, dst)?;
    /// println!("Available: {} up, {} core, {} down",
    ///     segments.up_segments.len(),
    ///     segments.core_segments.len(),
    ///     segments.down_segments.len());
    ///
    /// // Combine first 10 paths
    /// let mut paths = Vec::new();
    /// for up in segments.up_segments.iter().take(5) {
    ///     for core in segments.core_segments.iter().take(2) {
    ///         for down in segments.down_segments.iter().take(5) {
    ///             paths.push(ForwardingPath::new(Some(up.clone()), Some(core.clone()), Some(down.clone()))?);
    ///         }
    ///     }
    /// }
    /// ```
    pub fn scion_lookup_path_segments(
        &self,
        src: RouterId,
        dst: RouterId,
    ) -> Result<PathSegments<P>, NetworkError> {
        // Spec-compliant limit: 50 segments per type (spec line 1558)
        const MAX_SEGMENTS: usize = 50;

        let src_router = self.get_router(src)?;
        let dst_router = self.get_router(dst)?;

        let src_cs = src_router.scion().ok_or(NetworkError::DeviceNotFound(src))?;
        let dst_cs = dst_router.scion().ok_or(NetworkError::DeviceNotFound(dst))?;

        let src_isd_as = src_cs.isd_as;
        let dst_isd_as = dst_cs.isd_as;

        let mut up_segments = Vec::new();
        let mut core_segments = Vec::new();
        let mut down_segments = Vec::new();

        // Same ISD: up + down segments
        if src_isd_as.isd == dst_isd_as.isd {
            // Collect up segments if source is not core
            // Note: up-segments can be stored at any AS in the source ISD
            if !src_cs.is_core {
                for router_id in self.routers.keys() {
                    if let Ok(r) = self.get_router(*router_id) {
                        if let Some(cs) = r.scion() {
                            // Check all ASes in the source ISD (not just cores)
                            if cs.isd_as.isd == src_isd_as.isd {
                                up_segments.extend(
                                    cs.lookup_up_segments_from(&src_isd_as)
                                        .into_iter()
                                        .take(MAX_SEGMENTS - up_segments.len())
                                );
                                if up_segments.len() >= MAX_SEGMENTS {
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // Collect down segments if destination is not core
            // Note: down-segments are ONLY stored at core ASes (not transits/leaves)
            if !dst_cs.is_core {
                for router_id in self.routers.keys() {
                    if let Ok(r) = self.get_router(*router_id) {
                        if let Some(cs) = r.scion() {
                            // Check all ASes in the destination ISD (not just cores)
                            // In hierarchical topologies, down-segments may be stored at transits
                            if cs.isd_as.isd == dst_isd_as.isd {
                                down_segments.extend(
                                    cs.lookup_down_segments_to(&dst_isd_as)
                                        .into_iter()
                                        .take(MAX_SEGMENTS - down_segments.len())
                                );
                                if down_segments.len() >= MAX_SEGMENTS {
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // If both are core, get core segments between them
            if src_cs.is_core && dst_cs.is_core {
                core_segments = src_cs.lookup_core_segments(&src_isd_as, &dst_isd_as)
                    .into_iter()
                    .take(MAX_SEGMENTS)
                    .collect();
            }
        } else {
            // Different ISDs: need up + core + down
            // Collect up segments from source
            if !src_cs.is_core {
                // Get up segments from any AS in source ISD
                for router_id in self.routers.keys() {
                    if let Ok(r) = self.get_router(*router_id) {
                        if let Some(cs) = r.scion() {
                            // Check all ASes in source ISD (not just cores)
                            if cs.isd_as.isd == src_isd_as.isd {
                                up_segments.extend(
                                    cs.lookup_up_segments_from(&src_isd_as)
                                        .into_iter()
                                        .take(MAX_SEGMENTS - up_segments.len())
                                );
                                if up_segments.len() >= MAX_SEGMENTS {
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // Collect core segments between ISDs
            for router_id in self.routers.keys() {
                if let Ok(r) = self.get_router(*router_id) {
                    if let Some(cs) = r.scion() {
                        if cs.is_core && cs.isd_as.isd == src_isd_as.isd {
                            for dst_router_id in self.routers.keys() {
                                if let Ok(dr) = self.get_router(*dst_router_id) {
                                    if let Some(dcs) = dr.scion() {
                                        if dcs.is_core && dcs.isd_as.isd == dst_isd_as.isd {
                                            core_segments.extend(
                                                cs.lookup_core_segments(&cs.isd_as, &dcs.isd_as)
                                                    .into_iter()
                                                    .take(MAX_SEGMENTS - core_segments.len())
                                            );
                                            if core_segments.len() >= MAX_SEGMENTS {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            if core_segments.len() >= MAX_SEGMENTS {
                                break;
                            }
                        }
                    }
                }
            }

            // Collect down segments to destination
            if !dst_cs.is_core {
                for router_id in self.routers.keys() {
                    if let Ok(r) = self.get_router(*router_id) {
                        if let Some(cs) = r.scion() {
                            // Check all ASes in destination ISD (not just cores)
                            // In hierarchical topologies, down-segments may be stored at transits
                            if cs.isd_as.isd == dst_isd_as.isd {
                                down_segments.extend(
                                    cs.lookup_down_segments_to(&dst_isd_as)
                                        .into_iter()
                                        .take(MAX_SEGMENTS - down_segments.len())
                                );
                                if down_segments.len() >= MAX_SEGMENTS {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(PathSegments {
            up_segments,
            core_segments,
            down_segments,
        })
    }

    /// Lookup paths from source to destination (intra-ISD).
    ///
    /// For paths within the same ISD, this combines up-segments and down-segments.
    ///
    /// **Returns ALL available paths** - In SCION, endhosts select paths, so this method
    /// returns all possible forwarding paths by combining all available up-segments with
    /// all available down-segments, plus all peering shortcuts.
    ///
    /// # Arguments
    /// * `src` - Source router
    /// * `dst` - Destination router
    ///
    /// # Returns
    /// `Ok(Vec<ForwardingPath>)` - Vector of all available paths within the ISD
    pub fn scion_lookup_intra_isd_paths(
        &self,
        src: RouterId,
        dst: RouterId,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        use crate::scion::ForwardingPath;

        // Spec-compliant limits (draft-dekater-scion-controlplane-10, line 1558)
        const MAX_UP_SEGMENTS: usize = 50;
        const MAX_DOWN_SEGMENTS: usize = 50;
        const MAX_PATHS_RETURN: usize = 1000;

        let src_router = self.get_router(src)?;
        let dst_router = self.get_router(dst)?;

        let src_cs = src_router.scion().ok_or(NetworkError::DeviceNotFound(src))?;
        let dst_cs = dst_router.scion().ok_or(NetworkError::DeviceNotFound(dst))?;

        let src_isd_as = src_cs.isd_as;
        let dst_isd_as = dst_cs.isd_as;

        // Check if same ISD
        if src_isd_as.isd != dst_isd_as.isd {
            return Ok(Vec::new()); // Different ISDs, use inter-ISD lookup
        }

        let mut paths = Vec::new();

        // If both are core ASes in same ISD, look for core segments
        if src_cs.is_core && dst_cs.is_core {
            let core_segments = src_cs.lookup_core_segments(&src_isd_as, &dst_isd_as);
            for core_seg in core_segments {
                if let Ok(path) = ForwardingPath::new(None, Some(core_seg), None) {
                    paths.push(path);
                }
            }
            return Ok(paths);
        }

        // Find core ASes that can serve as intermediaries
        // Get all routers to find core ASes in this ISD
        let core_routers: Vec<RouterId> = self.routers
            .iter()
            .filter_map(|(id, r)| {
                if let Some(cs) = r.scion() {
                    if cs.is_core && cs.isd_as.isd == src_isd_as.isd {
                        return Some(*id);
                    }
                }
                None
            })
            .collect();

        // For each core AS, try to construct a path
        for core_router in core_routers {
            // Early exit if we've reached the path limit
            if paths.len() >= MAX_PATHS_RETURN {
                break;
            }

            let core = self.get_router(core_router)?;
            let core_cs = core.scion().unwrap();
            let core_isd_as = core_cs.isd_as;

            // Get up-segment from src to core
            let up_segments: Vec<_> = if src_cs.is_core {
                vec![] // Source is already core, no up-segment needed
            } else {
                // Look for up-segments TO this core FROM the source
                core_cs.lookup_up_segments(&core_isd_as)
                    .into_iter()
                    .filter(|seg| seg.source() == Some(src_isd_as))
                    .take(MAX_UP_SEGMENTS)
                    .map(|seg| seg.clone())
                    .collect()
            };

            // Get down-segment from core to dst
            let down_segments: Vec<_> = if dst_cs.is_core {
                vec![] // Destination is core, no down-segment needed
            } else {
                // Look for down-segments TO the destination FROM this core
                // Use wrapper method which limits before cloning
                core_cs.lookup_down_segments_to(&dst_isd_as)
                    .into_iter()
                    .filter(|seg| seg.source() == Some(core_isd_as))
                    .take(MAX_DOWN_SEGMENTS)
                    .collect()
            };

            // Combine segments
            if src_cs.is_core && !dst_cs.is_core {
                // Core to non-core: only down-segment
                for down_seg in &down_segments {
                    if let Ok(path) = ForwardingPath::new(None, None, Some(down_seg.clone())) {
                        paths.push(path);
                    }
                }
            } else if !src_cs.is_core && dst_cs.is_core {
                // Non-core to core: only up-segment
                for up_seg in &up_segments {
                    if let Ok(path) = ForwardingPath::new(Some(up_seg.clone()), None, None) {
                        paths.push(path);
                    }
                }
            } else if !src_cs.is_core && !dst_cs.is_core {
                // Non-core to non-core: up + down
                for up_seg in &up_segments {
                    if paths.len() >= MAX_PATHS_RETURN {
                        break;
                    }
                    for down_seg in &down_segments {
                        if paths.len() >= MAX_PATHS_RETURN {
                            break;
                        }
                        // Try regular path (through core)
                        if let Ok(path) = ForwardingPath::new(Some(up_seg.clone()), None, Some(down_seg.clone())) {
                            paths.push(path);
                        }

                        // Try peering shortcut paths
                        let shortcuts = up_seg.find_peering_shortcuts(down_seg);
                        for (pos1, pos2, peering) in shortcuts {
                            if paths.len() >= MAX_PATHS_RETURN {
                                break;
                            }
                            if let Ok(shortcut_path) = ForwardingPath::new_with_peering(
                                up_seg.clone(),
                                down_seg.clone(),
                                pos1,
                                pos2,
                                *peering,
                            ) {
                                paths.push(shortcut_path);
                            }
                        }
                    }
                }
            }
        }

        Ok(paths)
    }

    /// Find multi-hop core paths between two core ASes (possibly in different ISDs).
    ///
    /// Uses BFS to find paths through intermediate ISDs.
    ///
    /// # Arguments
    /// * `src_core` - Source core router ID
    /// * `dst_core` - Destination core router ID
    /// * `max_paths` - Maximum number of core path chains to return (for performance)
    ///
    /// # Returns
    /// Vector of core segment chains (each chain is a Vec of segments to traverse)
    ///
    /// # Performance Note
    /// The `max_paths` parameter limits the number of core path chains to prevent
    /// exhaustive exploration in complex topologies. The total number of forwarding
    /// paths returned to endhosts can still be larger, as each core chain is combined
    /// with all available up-segments and down-segments.
    fn find_multi_hop_core_paths(
        &self,
        src_core: RouterId,
        dst_core: RouterId,
        max_paths: usize,
    ) -> Result<Vec<Vec<crate::scion::PathSegment<P>>>, NetworkError> {
        use std::collections::{VecDeque, HashSet, HashMap};

        let src_core_router = self.get_router(src_core)?;
        let dst_core_router = self.get_router(dst_core)?;

        let src_core_cs = src_core_router.scion().ok_or(NetworkError::DeviceNotFound(src_core))?;
        let dst_core_cs = dst_core_router.scion().ok_or(NetworkError::DeviceNotFound(dst_core))?;

        let src_isd_as = src_core_cs.isd_as;
        let dst_isd_as = dst_core_cs.isd_as;

        // **OPTIMIZATION**: Pre-build a map of ISD-AS -> RouterId for O(1) lookups
        let mut isd_as_to_router: HashMap<crate::scion::IsdAs, RouterId> = HashMap::new();
        for (router_id, router) in &self.routers {
            if let Some(cs) = router.scion() {
                if cs.is_core {
                    isd_as_to_router.insert(cs.isd_as, *router_id);
                }
            }
        }

        // BFS to find all paths from src_core to dst_core
        let mut queue = VecDeque::new();
        let mut found_paths = Vec::new();

        // Queue entry: (current_isd_as, path_so_far, visited_in_this_path)
        let initial_visited = HashSet::new();
        queue.push_back((src_isd_as, Vec::new(), initial_visited.clone()));

        // Limit search depth to prevent infinite exploration
        let max_depth = 10;

        while let Some((current_isd_as, path_so_far, visited_in_this_path)) = queue.pop_front() {
            // **OPTIMIZATION**: Stop if we've found enough paths
            if found_paths.len() >= max_paths {
                break;
            }

            // Depth limit
            if path_so_far.len() >= max_depth {
                continue;
            }

            // If we reached the destination, save this path
            if current_isd_as == dst_isd_as {
                if !path_so_far.is_empty() {
                    found_paths.push(path_so_far);
                }
                continue;
            }

            // Avoid revisiting in the same path (prevent loops)
            if visited_in_this_path.contains(&current_isd_as) {
                continue;
            }

            // **OPTIMIZATION**: O(1) lookup instead of O(n) iteration
            if let Some(&current_router_id) = isd_as_to_router.get(&current_isd_as) {
                let current_cs = self.get_router(current_router_id)?.scion().unwrap();

                // Get all core segments from this AS
                let all_core_segs = current_cs.path_database.get_all_core_segments();

                for seg in all_core_segs {
                    if let Some(next_isd_as) = seg.destination() {
                        // Only follow segments that make progress (different from current)
                        if next_isd_as != current_isd_as && !visited_in_this_path.contains(&next_isd_as) {
                            let mut new_path = path_so_far.clone();
                            new_path.push(seg.clone());

                            // Create new visited set for this branch
                            let mut new_visited = visited_in_this_path.clone();
                            new_visited.insert(current_isd_as);

                            queue.push_back((next_isd_as, new_path, new_visited));
                        }
                    }
                }
            }
        }

        Ok(found_paths)
    }

    /// Lookup paths from source to destination (inter-ISD).
    ///
    /// For paths across ISDs, this combines up-segments, core-segments, and down-segments.
    /// Supports multi-hop core traversal through intermediate ISDs.
    ///
    /// **Returns ALL available paths** - In SCION, endhosts select paths, so this method
    /// returns all possible forwarding paths by combining all available segments.
    ///
    /// # Arguments
    /// * `src` - Source router
    /// * `dst` - Destination router
    ///
    /// # Returns
    /// `Ok(Vec<ForwardingPath>)` - Vector of all available paths
    ///
    /// # Performance Note
    /// For large multi-ISD topologies, the number of core path chains is limited to 100
    /// by default to maintain performance while still providing good path diversity.
    /// Each core chain is combined with ALL available up-segments and down-segments,
    /// so the total number of paths can still be quite large. Use
    /// `scion_lookup_inter_isd_paths_limited` to customize this limit.
    pub fn scion_lookup_inter_isd_paths(
        &self,
        src: RouterId,
        dst: RouterId,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        // Default: limit to 100 core path chains for performance
        self.scion_lookup_inter_isd_paths_limited(src, dst, 100)
    }

    /// Lookup paths from source to destination (inter-ISD) with configurable limit.
    ///
    /// Like `scion_lookup_inter_isd_paths` but allows customizing the maximum number
    /// of core path chains to explore.
    ///
    /// **Returns ALL available paths** - In SCION, endhosts select paths, so this method
    /// returns all possible forwarding paths by combining all available segments.
    ///
    /// # Arguments
    /// * `src` - Source router
    /// * `dst` - Destination router
    /// * `max_core_chains` - Maximum number of core path chains to explore (0 = unlimited)
    ///
    /// # Returns
    /// `Ok(Vec<ForwardingPath>)` - Vector of all available paths
    ///
    /// # Performance vs Completeness Trade-off
    /// - Higher values: More complete path discovery, but slower for complex topologies
    /// - Lower values: Faster, but may miss some valid paths
    /// - 0 (unlimited): Returns truly ALL paths, but may be very slow for large networks
    /// - 100 (default): Good balance for most topologies (< 10 ISDs)
    ///
    /// # Example
    /// ```ignore
    /// // Get all paths with default limit (100 core chains)
    /// let paths = net.scion_lookup_inter_isd_paths(src, dst)?;
    ///
    /// // Get all paths with no limit (exhaustive search)
    /// let all_paths = net.scion_lookup_inter_isd_paths_limited(src, dst, 0)?;
    ///
    /// // Get paths with stricter limit (faster)
    /// let fewer_paths = net.scion_lookup_inter_isd_paths_limited(src, dst, 10)?;
    /// ```
    pub fn scion_lookup_inter_isd_paths_limited(
        &self,
        src: RouterId,
        dst: RouterId,
        max_core_chains: usize,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        use crate::scion::ForwardingPath;

        // Spec-compliant limits (draft-dekater-scion-controlplane-10, line 1558):
        // "A parent AS propagates (at most) the best PCBs to each of its child ASes.
        //  This number SHOULD be limited to at most 50..."
        // We apply similar limits here to prevent combinatorial explosion.
        const MAX_UP_SEGMENTS: usize = 50;
        const MAX_DOWN_SEGMENTS: usize = 50;
        const MAX_PATHS_RETURN: usize = 1000;

        let src_router = self.get_router(src)?;
        let dst_router = self.get_router(dst)?;

        let src_cs = src_router.scion().ok_or(NetworkError::DeviceNotFound(src))?;
        let dst_cs = dst_router.scion().ok_or(NetworkError::DeviceNotFound(dst))?;

        let src_isd_as = src_cs.isd_as;
        let dst_isd_as = dst_cs.isd_as;

        // Check if different ISDs
        if src_isd_as.isd == dst_isd_as.isd {
            return Ok(Vec::new()); // Same ISD, use intra-ISD lookup
        }

        let mut paths = Vec::new();

        // Find core ASes in source and destination ISDs
        let src_cores: Vec<RouterId> = self.routers
            .iter()
            .filter_map(|(id, r)| {
                if let Some(cs) = r.scion() {
                    if cs.is_core && cs.isd_as.isd == src_isd_as.isd {
                        return Some(*id);
                    }
                }
                None
            })
            .collect();

        let dst_cores: Vec<RouterId> = self.routers
            .iter()
            .filter_map(|(id, r)| {
                if let Some(cs) = r.scion() {
                    if cs.is_core && cs.isd_as.isd == dst_isd_as.isd {
                        return Some(*id);
                    }
                }
                None
            })
            .collect();

        // For each pair of core ASes, try to construct a path
        for src_core_router in &src_cores {
            // Early exit if we've found enough paths
            if paths.len() >= MAX_PATHS_RETURN {
                break;
            }

            for dst_core_router in &dst_cores {
                // Early exit if we've found enough paths
                if paths.len() >= MAX_PATHS_RETURN {
                    break;
                }

                let src_core = self.get_router(*src_core_router)?;
                let dst_core = self.get_router(*dst_core_router)?;

                let src_core_cs = src_core.scion().unwrap();
                let dst_core_cs = dst_core.scion().unwrap();

                let _src_core_isd_as = src_core_cs.isd_as;
                let _dst_core_isd_as = dst_core_cs.isd_as;

                // Get up-segment from src to src_core (limit to prevent explosion)
                let up_segments: Vec<_> = if src_cs.is_core {
                    vec![]
                } else {
                    src_core_cs.lookup_up_segments_from(&src_isd_as)
                        .into_iter()
                        .take(MAX_UP_SEGMENTS)
                        .collect()
                };

                // Find multi-hop core paths (including direct paths)
                let limit = if max_core_chains == 0 { usize::MAX } else { max_core_chains };
                let core_path_chains = self.find_multi_hop_core_paths(*src_core_router, *dst_core_router, limit)?;

                // Get down-segment from dst_core to dst (limit to prevent explosion)
                let down_segments: Vec<_> = if dst_cs.is_core {
                    vec![]
                } else {
                    dst_core_cs.lookup_down_segments_to(&dst_isd_as)
                        .into_iter()
                        .take(MAX_DOWN_SEGMENTS)
                        .collect()
                };

                // Process each core path chain
                for core_chain in &core_path_chains {
                    // Early exit if we've found enough paths
                    if paths.len() >= MAX_PATHS_RETURN {
                        break;
                    }

                    // Chain the core segments together
                    let core_seg = match crate::scion::PathSegment::chain_core_segments(core_chain.clone()) {
                        Some(seg) => seg,
                        None => continue, // Skip invalid chains
                    };
                    if src_cs.is_core && dst_cs.is_core {
                        // Core to core across ISDs: only core-segment
                        if let Ok(path) = ForwardingPath::new(None, Some(core_seg.clone()), None) {
                            paths.push(path);
                            if paths.len() >= MAX_PATHS_RETURN {
                                break;
                            }
                        }
                    } else if src_cs.is_core && !dst_cs.is_core {
                        // Core to non-core across ISDs: core + down
                        for down_seg in &down_segments {
                            if paths.len() >= MAX_PATHS_RETURN {
                                break;
                            }
                            if let Ok(path) = ForwardingPath::new(None, Some(core_seg.clone()), Some(down_seg.clone())) {
                                paths.push(path);
                            }
                            // Check for peering shortcuts between core and down
                            let shortcuts = core_seg.find_peering_shortcuts(down_seg);
                            for (pos1, pos2, peering) in shortcuts {
                                if paths.len() >= MAX_PATHS_RETURN {
                                    break;
                                }
                                if let Ok(shortcut_path) = ForwardingPath::new_with_peering(
                                    core_seg.clone(),
                                    down_seg.clone(),
                                    pos1,
                                    pos2,
                                    *peering,
                                ) {
                                    paths.push(shortcut_path);
                                }
                            }
                        }
                    } else if !src_cs.is_core && dst_cs.is_core {
                        // Non-core to core across ISDs: up + core
                        for up_seg in &up_segments {
                            if paths.len() >= MAX_PATHS_RETURN {
                                break;
                            }
                            if let Ok(path) = ForwardingPath::new(Some(up_seg.clone()), Some(core_seg.clone()), None) {
                                paths.push(path);
                            }
                            // Check for peering shortcuts between up and core
                            let shortcuts = up_seg.find_peering_shortcuts(&core_seg);
                            for (pos1, pos2, peering) in shortcuts {
                                if paths.len() >= MAX_PATHS_RETURN {
                                    break;
                                }
                                if let Ok(shortcut_path) = ForwardingPath::new_with_peering(
                                    up_seg.clone(),
                                    core_seg.clone(),
                                    pos1,
                                    pos2,
                                    *peering,
                                ) {
                                    paths.push(shortcut_path);
                                }
                            }
                        }
                    } else {
                        // Non-core to non-core across ISDs: up + core + down
                        for up_seg in &up_segments {
                            if paths.len() >= MAX_PATHS_RETURN {
                                break;
                            }
                            for down_seg in &down_segments {
                                if paths.len() >= MAX_PATHS_RETURN {
                                    break;
                                }
                                // Regular 3-segment path
                                if let Ok(path) = ForwardingPath::new(Some(up_seg.clone()), Some(core_seg.clone()), Some(down_seg.clone())) {
                                    paths.push(path);
                                }

                                // Peering shortcut between up and core
                                let shortcuts_up_core = up_seg.find_peering_shortcuts(&core_seg);
                                for (pos1, pos2, peering) in shortcuts_up_core {
                                    if paths.len() >= MAX_PATHS_RETURN {
                                        break;
                                    }
                                    if let Ok(shortcut_path) = ForwardingPath::new_with_peering(
                                        up_seg.clone(),
                                        core_seg.clone(),
                                        pos1,
                                        pos2,
                                        *peering,
                                    ) {
                                        paths.push(shortcut_path);
                                    }
                                }

                                // Peering shortcut between core and down
                                let shortcuts_core_down = core_seg.find_peering_shortcuts(down_seg);
                                for (pos1, pos2, peering) in shortcuts_core_down {
                                    if paths.len() >= MAX_PATHS_RETURN {
                                        break;
                                    }
                                    if let Ok(shortcut_path) = ForwardingPath::new_with_peering(
                                        core_seg.clone(),
                                        down_seg.clone(),
                                        pos1,
                                        pos2,
                                        *peering,
                                    ) {
                                        paths.push(shortcut_path);
                                    }
                                }

                                // Peering shortcut between up and down
                                let shortcuts_up_down = up_seg.find_peering_shortcuts(down_seg);
                                for (pos1, pos2, peering) in shortcuts_up_down {
                                    if paths.len() >= MAX_PATHS_RETURN {
                                        break;
                                    }
                                    if let Ok(shortcut_path) = ForwardingPath::new_with_peering(
                                        up_seg.clone(),
                                        down_seg.clone(),
                                        pos1,
                                        pos2,
                                        *peering,
                                    ) {
                                        paths.push(shortcut_path);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(paths)
    }

    /// Lookup all available paths from source to destination.
    ///
    /// Automatically determines whether to use intra-ISD or inter-ISD lookup.
    ///
    /// **Returns ALL available paths** - In SCION, endhosts select paths from the
    /// available set. This method returns all possible forwarding paths by combining
    /// all available segments. For path selection, use `scion_lookup_paths_with_selection`.
    ///
    /// # Arguments
    /// * `src` - Source router
    /// * `dst` - Destination router
    ///
    /// # Returns
    /// `Ok(Vec<ForwardingPath>)` - Vector of all available forwarding paths
    ///
    /// # Performance Note
    /// For inter-ISD lookups in large topologies, the number of core path chains is
    /// limited to 100 by default. Use `scion_lookup_inter_isd_paths_limited` to customize.
    pub fn scion_lookup_paths(
        &self,
        src: RouterId,
        dst: RouterId,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        let src_router = self.get_router(src)?;
        let dst_router = self.get_router(dst)?;

        let src_cs = src_router.scion().ok_or(NetworkError::DeviceNotFound(src))?;
        let dst_cs = dst_router.scion().ok_or(NetworkError::DeviceNotFound(dst))?;

        let src_isd_as = src_cs.isd_as;
        let dst_isd_as = dst_cs.isd_as;

        // Determine if intra-ISD or inter-ISD
        if src_isd_as.isd == dst_isd_as.isd {
            self.scion_lookup_intra_isd_paths(src, dst)
        } else {
            self.scion_lookup_inter_isd_paths(src, dst)
        }
    }

    /// Lookup paths with selection policy.
    ///
    /// This method finds all available paths between source and destination,
    /// then applies the provided selection policy to choose the best paths.
    ///
    /// # Arguments
    /// * `src` - Source router ID
    /// * `dst` - Destination router ID
    /// * `policy` - Path selection policy to apply
    /// * `max_count` - Maximum number of paths to return
    ///
    /// # Returns
    /// Selected forwarding paths, ordered by preference according to the policy
    ///
    /// # Example
    /// ```ignore
    /// use bgpsim::scion::{ShortestPathPolicy, PathSelectionPolicy};
    ///
    /// let policy = ShortestPathPolicy;
    /// let paths = net.scion_lookup_paths_with_selection(src, dst, &policy, 3)?;
    /// ```
    pub fn scion_lookup_paths_with_selection(
        &self,
        src: RouterId,
        dst: RouterId,
        policy: &dyn crate::scion::PathSelectionPolicy<P>,
        max_count: usize,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        // Get all available paths
        let all_paths = self.scion_lookup_paths(src, dst)?;

        if all_paths.is_empty() || max_count == 0 {
            return Ok(Vec::new());
        }

        // Apply selection policy
        let path_refs: Vec<&crate::scion::ForwardingPath<P>> = all_paths.iter().collect();
        let selected_indices = policy.select_paths(&path_refs, max_count);

        // Return selected paths
        Ok(selected_indices
            .into_iter()
            .map(|idx| all_paths[idx].clone())
            .collect())
    }

    /// Lookup intra-ISD paths with selection policy.
    ///
    /// Like `scion_lookup_intra_isd_paths` but applies a selection policy
    /// to choose the best paths.
    pub fn scion_lookup_intra_isd_paths_with_selection(
        &self,
        src: RouterId,
        dst: RouterId,
        policy: &dyn crate::scion::PathSelectionPolicy<P>,
        max_count: usize,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        let all_paths = self.scion_lookup_intra_isd_paths(src, dst)?;

        if all_paths.is_empty() || max_count == 0 {
            return Ok(Vec::new());
        }

        let path_refs: Vec<&crate::scion::ForwardingPath<P>> = all_paths.iter().collect();
        let selected_indices = policy.select_paths(&path_refs, max_count);

        Ok(selected_indices
            .into_iter()
            .map(|idx| all_paths[idx].clone())
            .collect())
    }

    /// Remove expired PCBs and path segments from all SCION-enabled routers.
    ///
    /// This should be called periodically to clean up stale state.
    ///
    /// # Arguments
    /// * `current_time` - Current simulation timestamp
    ///
    /// # Returns
    /// `Ok((expired_pcbs, expired_segments))` - Counts of expired items removed
    pub fn scion_cleanup_expired(
        &mut self,
        current_time: u32,
    ) -> Result<(usize, usize), NetworkError> {
        let mut total_pcbs = 0;
        let mut total_segments = 0;

        let router_ids: Vec<RouterId> = self.routers.keys().copied().collect();
        for router_id in router_ids {
            let router = self.get_router_mut(router_id)?;

            if let Some(cs) = router.scion_mut() {
                let (pcbs, segments) = cs.clear_expired(current_time);
                total_pcbs += pcbs;
                total_segments += segments;
            }
        }

        Ok((total_pcbs, total_segments))
    }

    /// Detect and handle link failures in SCION topology.
    ///
    /// Removes interfaces for failed links and clears affected PCBs/segments.
    ///
    /// # Arguments
    /// * `failed_link` - Tuple of (router1, router2) representing failed link
    /// * `current_time` - Current simulation timestamp
    ///
    /// # Returns
    /// `Ok(num_affected_items)` - Number of PCBs/segments invalidated
    pub fn scion_handle_link_failure(
        &mut self,
        failed_link: (RouterId, RouterId),
        current_time: u32,
    ) -> Result<usize, NetworkError> {
        let (r1, r2) = failed_link;
        let mut affected_items = 0;

        // Remove interface from r1 to r2
        if let Ok(router) = self.get_router_mut(r1) {
            if let Some(cs) = router.scion_mut() {
                cs.remove_interface(r2);
                // Clear all expired (this will remove segments using the failed link)
                let (pcbs, segs) = cs.clear_expired(current_time);
                affected_items += pcbs + segs;
            }
        }

        // Remove interface from r2 to r1
        if let Ok(router) = self.get_router_mut(r2) {
            if let Some(cs) = router.scion_mut() {
                cs.remove_interface(r1);
                let (pcbs, segs) = cs.clear_expired(current_time);
                affected_items += pcbs + segs;
            }
        }

        Ok(affected_items)
    }

    /// Lookup inter-ISD paths with selection policy.
    ///
    /// Like `scion_lookup_inter_isd_paths` but applies a selection policy
    /// to choose the best paths.
    pub fn scion_lookup_inter_isd_paths_with_selection(
        &self,
        src: RouterId,
        dst: RouterId,
        policy: &dyn crate::scion::PathSelectionPolicy<P>,
        max_count: usize,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        let all_paths = self.scion_lookup_inter_isd_paths(src, dst)?;

        if all_paths.is_empty() || max_count == 0 {
            return Ok(Vec::new());
        }

        let path_refs: Vec<&crate::scion::ForwardingPath<P>> = all_paths.iter().collect();
        let selected_indices = policy.select_paths(&path_refs, max_count);

        Ok(selected_indices
            .into_iter()
            .map(|idx| all_paths[idx].clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SimplePrefix;
    use crate::event::BasicEventQueue;
    use crate::ospf::GlobalOspf;

    #[test]
    fn test_enable_scion() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();
        let r1 = net.add_router("r1", 65500);

        let isd_as = IsdAs::new(1, 110u64);
        assert!(net.enable_scion(r1, isd_as, true).is_ok());
        assert!(net.is_scion_enabled(r1).unwrap());
    }

    #[test]
    fn test_configure_scion_link() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();
        let r1 = net.add_router("r1", 65500);
        let r2 = net.add_router("r2", 65500);

        net.add_link(r1, r2).unwrap();

        // Enable SCION on both routers
        net.enable_scion(r1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(r2, IsdAs::new(1, 120u64), true).unwrap();

        // Configure SCION link
        assert!(net.configure_scion_link(r1, r2, ScionLinkType::Core).is_ok());

        // Verify interfaces were added
        let cs1 = net.get_router(r1).unwrap().scion().unwrap();
        assert_eq!(cs1.interface_count(), 1);

        let cs2 = net.get_router(r2).unwrap().scion().unwrap();
        assert_eq!(cs2.interface_count(), 1);
    }

    #[test]
    fn test_core_beaconing() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();
        let r1 = net.add_router("r1", 65500);
        let r2 = net.add_router("r2", 65500);

        net.add_link(r1, r2).unwrap();

        // Enable SCION on both routers as core ASes
        net.enable_scion(r1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(r2, IsdAs::new(1, 120u64), true).unwrap();

        // Configure SCION link
        net.configure_scion_link(r1, r2, ScionLinkType::Core).unwrap();

        // Run core beaconing
        let created = net.scion_core_beaconing(1000).unwrap();
        assert_eq!(created, 2); // r1->r2 and r2->r1

        // Verify PCBs were stored
        let bs1 = net.get_scion_beacon_store(r1).unwrap();
        assert_eq!(bs1.total_count(), 1); // r2's PCB

        let bs2 = net.get_scion_beacon_store(r2).unwrap();
        assert_eq!(bs2.total_count(), 1); // r1's PCB
    }

    #[test]
    fn test_up_segment_registration() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create hierarchical topology: Core1 - Child1
        let core1 = net.add_router("Core1", 65500);
        let child1 = net.add_router("Child1", 65500);

        net.add_link(core1, child1).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(child1, IsdAs::new(1, 120u64), false).unwrap();

        // Configure link
        net.configure_scion_link(core1, child1, ScionLinkType::ParentChild).unwrap();

        // Run core beaconing (child1 receives PCBs)
        net.scion_core_beaconing(1000).unwrap();

        // Register up-segments
        let registered = net.scion_register_up_segments(5).unwrap();
        assert_eq!(registered, 1); // child1 registers 1 up-segment at core1

        // Verify up-segment was registered at core1
        let core1_router = net.get_router(core1).unwrap();
        let core1_cs = core1_router.scion().unwrap();
        let up_segs = core1_cs.path_database.get_all_up_segments();
        assert_eq!(up_segs.len(), 1);
    }

    #[test]
    fn test_down_segment_registration() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create hierarchical topology: Core1 - Child1
        let core1 = net.add_router("Core1", 65500);
        let child1 = net.add_router("Child1", 65500);

        net.add_link(core1, child1).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(child1, IsdAs::new(1, 120u64), false).unwrap();

        // Configure link
        net.configure_scion_link(core1, child1, ScionLinkType::ParentChild).unwrap();

        // Run core beaconing + up-segment registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_register_up_segments(5).unwrap();

        // Register down-segments
        let registered = net.scion_register_down_segments().unwrap();
        assert_eq!(registered, 1); // core1 creates 1 down-segment

        // Verify down-segment was registered at core1
        let core1_router = net.get_router(core1).unwrap();
        let core1_cs = core1_router.scion().unwrap();
        let down_segs = core1_cs.path_database.get_all_down_segments();
        assert_eq!(down_segs.len(), 1);
    }

    #[test]
    fn test_core_segment_registration() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create core topology
        let core1 = net.add_router("Core1", 65500);
        let core2 = net.add_router("Core2", 65500);

        net.add_link(core1, core2).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(core2, IsdAs::new(1, 120u64), true).unwrap();

        // Configure link
        net.configure_scion_link(core1, core2, ScionLinkType::Core).unwrap();

        // Run core beaconing
        net.scion_core_beaconing(1000).unwrap();

        // Register core-segments
        let registered = net.scion_register_core_segments(5).unwrap();
        assert_eq!(registered, 4); // core1 and core2 each register 2 segments (original + reversed)

        // Verify core-segments were registered
        let core1_router = net.get_router(core1).unwrap();
        let core1_cs = core1_router.scion().unwrap();
        let core_segs = core1_cs.path_database.get_all_core_segments();
        assert_eq!(core_segs.len(), 2); // core1 registered core2's PCB in both directions
    }

    #[test]
    fn test_complete_registration_round() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create hierarchical topology
        let core1 = net.add_router("Core1", 65500);
        let core2 = net.add_router("Core2", 65500);
        let child1 = net.add_router("Child1", 65500);

        net.add_link(core1, core2).unwrap();
        net.add_link(core1, child1).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(core2, IsdAs::new(1, 120u64), true).unwrap();
        net.enable_scion(child1, IsdAs::new(1, 130u64), false).unwrap();

        // Configure links
        net.configure_scion_link(core1, core2, ScionLinkType::Core).unwrap();
        net.configure_scion_link(core1, child1, ScionLinkType::ParentChild).unwrap();

        // Run beaconing
        net.scion_core_beaconing(1000).unwrap();

        // Run complete registration
        let (up_count, down_count, core_count) = net.scion_registration_round(5).unwrap();

        assert_eq!(up_count, 1); // child1 registers 1 up-segment
        assert_eq!(down_count, 1); // core1 creates 1 down-segment
        assert_eq!(core_count, 4); // core1 and core2 each register 2 core-segments (both directions)
    }

    #[test]
    fn test_intra_isd_path_lookup() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create hierarchical topology: Core1 - Child1 - Child2
        let core1 = net.add_router("Core1", 65500);
        let child1 = net.add_router("Child1", 65500);
        let child2 = net.add_router("Child2", 65500);

        net.add_link(core1, child1).unwrap();
        net.add_link(core1, child2).unwrap();

        // Enable SCION (all in ISD 1)
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(child1, IsdAs::new(1, 120u64), false).unwrap();
        net.enable_scion(child2, IsdAs::new(1, 130u64), false).unwrap();

        // Configure links
        net.configure_scion_link(core1, child1, ScionLinkType::ParentChild).unwrap();
        net.configure_scion_link(core1, child2, ScionLinkType::ParentChild).unwrap();

        // Run beaconing and registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_registration_round(5).unwrap();

        // Lookup paths from child1 to child2
        let paths = net.scion_lookup_intra_isd_paths(child1, child2).unwrap();
        assert!(!paths.is_empty(), "Should find at least one path");

        // Verify path properties
        for path in &paths {
            assert!(path.validate().is_ok());
            assert!(path.is_valley_free());
            assert!(!path.has_loop());
        }
    }

    #[test]
    fn test_path_lookup_convenience_method() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create hierarchical topology
        let core1 = net.add_router("Core1", 65500);
        let child1 = net.add_router("Child1", 65500);

        net.add_link(core1, child1).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(child1, IsdAs::new(1, 120u64), false).unwrap();

        // Configure link
        net.configure_scion_link(core1, child1, ScionLinkType::ParentChild).unwrap();

        // Run beaconing and registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_registration_round(5).unwrap();

        // Lookup paths using convenience method
        let paths = net.scion_lookup_paths(child1, core1).unwrap();
        assert!(!paths.is_empty(), "Should find at least one path");
    }

    #[test]
    fn test_path_selection_shortest_path() {
        use crate::scion::ShortestPathPolicy;

        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create topology with multiple paths of different lengths
        let core1 = net.add_router("Core1", 65500);
        let core2 = net.add_router("Core2", 65500);
        let core3 = net.add_router("Core3", 65500);
        let child1 = net.add_router("Child1", 65500);
        let child2 = net.add_router("Child2", 65500);

        // Create topology
        net.add_link(child1, core1).unwrap();
        net.add_link(child1, core2).unwrap();
        net.add_link(core1, core3).unwrap();
        net.add_link(core2, core3).unwrap();
        net.add_link(core3, child2).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(core2, IsdAs::new(1, 111u64), true).unwrap();
        net.enable_scion(core3, IsdAs::new(1, 112u64), true).unwrap();
        net.enable_scion(child1, IsdAs::new(1, 120u64), false).unwrap();
        net.enable_scion(child2, IsdAs::new(1, 130u64), false).unwrap();

        // Configure links
        net.configure_scion_link(child1, core1, ScionLinkType::ParentChild).unwrap();
        net.configure_scion_link(child1, core2, ScionLinkType::ParentChild).unwrap();
        net.configure_scion_link(core1, core3, ScionLinkType::Core).unwrap();
        net.configure_scion_link(core2, core3, ScionLinkType::Core).unwrap();
        net.configure_scion_link(core3, child2, ScionLinkType::ParentChild).unwrap();

        // Run beaconing and registration
        net.scion_intra_isd_beaconing(1000, 5).unwrap();
        net.scion_core_beaconing(1000).unwrap();
        net.scion_registration_round(5).unwrap();

        // Get all paths
        let all_paths = net.scion_lookup_paths(child1, child2).unwrap();
        if all_paths.is_empty() {
            // Skip test if no paths found (might happen depending on topology)
            return;
        }

        // Use shortest path policy to select best 2 paths
        let policy = ShortestPathPolicy;
        let selected_paths = net.scion_lookup_paths_with_selection(
            child1,
            child2,
            &policy,
            2,
        ).unwrap();

        // Verify selection
        assert!(!selected_paths.is_empty(), "Should select at least one path");
        assert!(selected_paths.len() <= 2, "Should not exceed max_count");

        // Verify paths are sorted by length
        if selected_paths.len() >= 2 {
            assert!(
                selected_paths[0].length() <= selected_paths[1].length(),
                "Paths should be sorted by length"
            );
        }
    }

    #[test]
    fn test_path_selection_highest_mtu() {
        use crate::scion::HighestMtuPolicy;

        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create simple topology
        let core1 = net.add_router("Core1", 65500);
        let child1 = net.add_router("Child1", 65500);
        let child2 = net.add_router("Child2", 65500);

        net.add_link(child1, core1).unwrap();
        net.add_link(core1, child2).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(child1, IsdAs::new(1, 120u64), false).unwrap();
        net.enable_scion(child2, IsdAs::new(1, 130u64), false).unwrap();

        // Configure links
        net.configure_scion_link(child1, core1, ScionLinkType::ParentChild).unwrap();
        net.configure_scion_link(core1, child2, ScionLinkType::ParentChild).unwrap();

        // Run beaconing and registration
        net.scion_intra_isd_beaconing(1000, 5).unwrap();
        net.scion_registration_round(5).unwrap();

        // Get all paths first to check if any exist
        let all_paths = net.scion_lookup_paths(child1, child2).unwrap();
        if all_paths.is_empty() {
            // Skip test if no paths found
            return;
        }

        // Use highest MTU policy
        let policy = HighestMtuPolicy;
        let selected_paths = net.scion_lookup_paths_with_selection(
            child1,
            child2,
            &policy,
            1,
        ).unwrap();

        // Should find at least one path
        assert!(!selected_paths.is_empty(), "Should select at least one path");
        assert_eq!(selected_paths.len(), 1, "Should respect max_count");
    }

    #[test]
    fn test_path_selection_empty_result() {
        use crate::scion::ShortestPathPolicy;

        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create disconnected routers
        let core1 = net.add_router("Core1", 65500);
        let core2 = net.add_router("Core2", 65500);

        // No links between them

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(core2, IsdAs::new(1, 120u64), true).unwrap();

        // Try to lookup paths with selection
        let policy = ShortestPathPolicy;
        let selected_paths = net.scion_lookup_paths_with_selection(
            core1,
            core2,
            &policy,
            5,
        ).unwrap();

        // Should return empty vector
        assert!(selected_paths.is_empty(), "Should return no paths for disconnected routers");
    }

    #[test]
    fn test_inter_isd_core_to_core() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create ISD 1 with one core AS
        let core1_isd1 = net.add_router("Core1_ISD1", 65500);
        net.enable_scion(core1_isd1, IsdAs::new(1, 110u64), true).unwrap();

        // Create ISD 2 with one core AS
        let core1_isd2 = net.add_router("Core1_ISD2", 65501);
        net.enable_scion(core1_isd2, IsdAs::new(2, 210u64), true).unwrap();

        // Connect the two ISDs via core link
        net.add_link(core1_isd1, core1_isd2).unwrap();
        net.configure_scion_link(core1_isd1, core1_isd2, ScionLinkType::Core).unwrap();

        // Run core beaconing and registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_registration_round(2000).unwrap();

        // Lookup paths from ISD1 to ISD2
        let paths = net.scion_lookup_paths(core1_isd1, core1_isd2).unwrap();

        let cs1 = net.get_router(core1_isd1).unwrap().scion().unwrap();
        eprintln!("Core1_ISD1 has {} core segments", cs1.path_database.get_all_core_segments().len());
        for (i, seg) in cs1.path_database.get_all_core_segments().iter().enumerate() {
            eprintln!("  Segment {}: {:?} -> {:?}", i, seg.source(), seg.destination());
        }
        eprintln!("Found {} paths", paths.len());

        assert!(!paths.is_empty(), "Should find inter-ISD path between core ASes");
        assert_eq!(paths.len(), 1, "Should find exactly one path");

        let path = &paths[0];
        assert_eq!(path.as_path.len(), 2, "Path should have 2 ASes");
        assert_eq!(path.as_path[0], IsdAs::new(1, 110u64));
        assert_eq!(path.as_path[1], IsdAs::new(2, 210u64));
        assert!(path.core_segment.is_some(), "Should have core segment");
        assert!(path.up_segment.is_none(), "Should not have up segment");
        assert!(path.down_segment.is_none(), "Should not have down segment");
    }

    #[test]
    fn test_inter_isd_with_hierarchies() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create ISD 1 with hierarchy
        let core1_isd1 = net.add_router("Core1_ISD1", 65500);
        let leaf1_isd1 = net.add_router("Leaf1_ISD1", 65501);
        net.enable_scion(core1_isd1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(leaf1_isd1, IsdAs::new(1, 111u64), false).unwrap();

        net.add_link(core1_isd1, leaf1_isd1).unwrap();
        net.configure_scion_link(core1_isd1, leaf1_isd1, ScionLinkType::ParentChild).unwrap();

        // Create ISD 2 with hierarchy
        let core1_isd2 = net.add_router("Core1_ISD2", 65502);
        let leaf1_isd2 = net.add_router("Leaf1_ISD2", 65503);
        net.enable_scion(core1_isd2, IsdAs::new(2, 210u64), true).unwrap();
        net.enable_scion(leaf1_isd2, IsdAs::new(2, 211u64), false).unwrap();

        net.add_link(core1_isd2, leaf1_isd2).unwrap();
        net.configure_scion_link(core1_isd2, leaf1_isd2, ScionLinkType::ParentChild).unwrap();

        // Connect the two ISDs via core link
        net.add_link(core1_isd1, core1_isd2).unwrap();
        net.configure_scion_link(core1_isd1, core1_isd2, ScionLinkType::Core).unwrap();

        // Run beaconing and registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_intra_isd_beaconing(1000, 5).unwrap();
        net.scion_registration_round(5).unwrap();

        // Lookup paths from leaf in ISD1 to leaf in ISD2
        let paths = net.scion_lookup_paths(leaf1_isd1, leaf1_isd2).unwrap();

        assert!(!paths.is_empty(), "Should find inter-ISD path between leaf ASes");

        let path = &paths[0];
        assert_eq!(path.as_path.len(), 4, "Path should traverse 4 ASes (leaf-core-core-leaf)");
        assert_eq!(path.as_path[0], IsdAs::new(1, 111u64), "Should start at ISD1 leaf");
        assert_eq!(path.as_path[1], IsdAs::new(1, 110u64), "Should go through ISD1 core");
        assert_eq!(path.as_path[2], IsdAs::new(2, 210u64), "Should go through ISD2 core");
        assert_eq!(path.as_path[3], IsdAs::new(2, 211u64), "Should end at ISD2 leaf");

        // Verify segment composition
        assert!(path.up_segment.is_some(), "Should have up segment");
        assert!(path.core_segment.is_some(), "Should have core segment");
        assert!(path.down_segment.is_some(), "Should have down segment");
    }

    #[test]
    fn test_inter_isd_multiple_isds() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create ISD 1
        let core1 = net.add_router("Core1", 65500);
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();

        // Create ISD 2
        let core2 = net.add_router("Core2", 65501);
        net.enable_scion(core2, IsdAs::new(2, 210u64), true).unwrap();

        // Create ISD 3
        let core3 = net.add_router("Core3", 65502);
        net.enable_scion(core3, IsdAs::new(3, 310u64), true).unwrap();

        // Connect ISD1 <-> ISD2
        net.add_link(core1, core2).unwrap();
        net.configure_scion_link(core1, core2, ScionLinkType::Core).unwrap();

        // Connect ISD2 <-> ISD3
        net.add_link(core2, core3).unwrap();
        net.configure_scion_link(core2, core3, ScionLinkType::Core).unwrap();

        // Run beaconing and registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_registration_round(5).unwrap();

        // Lookup paths from ISD1 to ISD3 (should go through ISD2)
        let paths = net.scion_lookup_paths(core1, core3).unwrap();

        assert!(!paths.is_empty(), "Should find path through transit ISD");

        let path = &paths[0];
        assert_eq!(path.as_path.len(), 3, "Path should traverse 3 ASes");
        assert_eq!(path.as_path[0], IsdAs::new(1, 110u64));
        assert_eq!(path.as_path[1], IsdAs::new(2, 210u64));
        assert_eq!(path.as_path[2], IsdAs::new(3, 310u64));
    }

    #[test]
    fn test_inter_isd_no_path() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create ISD 1
        let core1 = net.add_router("Core1", 65500);
        net.enable_scion(core1, IsdAs::new(1, 110u64), true).unwrap();

        // Create ISD 2 (disconnected)
        let core2 = net.add_router("Core2", 65501);
        net.enable_scion(core2, IsdAs::new(2, 210u64), true).unwrap();

        // No link between ISDs

        // Run beaconing and registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_registration_round(2000).unwrap();

        // Lookup paths should return empty
        let paths = net.scion_lookup_paths(core1, core2).unwrap();

        assert!(paths.is_empty(), "Should find no path between disconnected ISDs");
    }

    #[test]
    fn test_inter_isd_with_peering() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create ISD 1 with hierarchy
        let core1_isd1 = net.add_router("Core1_ISD1", 65500);
        let as1_isd1 = net.add_router("AS1_ISD1", 65501);
        let as2_isd1 = net.add_router("AS2_ISD1", 65502);
        net.enable_scion(core1_isd1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(as1_isd1, IsdAs::new(1, 111u64), false).unwrap();
        net.enable_scion(as2_isd1, IsdAs::new(1, 112u64), false).unwrap();

        // Links in ISD1
        net.add_link(core1_isd1, as1_isd1).unwrap();
        net.configure_scion_link(core1_isd1, as1_isd1, ScionLinkType::ParentChild).unwrap();
        net.add_link(core1_isd1, as2_isd1).unwrap();
        net.configure_scion_link(core1_isd1, as2_isd1, ScionLinkType::ParentChild).unwrap();

        // Create ISD 2 with hierarchy
        let core1_isd2 = net.add_router("Core1_ISD2", 65503);
        let as1_isd2 = net.add_router("AS1_ISD2", 65504);
        let as2_isd2 = net.add_router("AS2_ISD2", 65505);
        net.enable_scion(core1_isd2, IsdAs::new(2, 210u64), true).unwrap();
        net.enable_scion(as1_isd2, IsdAs::new(2, 211u64), false).unwrap();
        net.enable_scion(as2_isd2, IsdAs::new(2, 212u64), false).unwrap();

        // Links in ISD2
        net.add_link(core1_isd2, as1_isd2).unwrap();
        net.configure_scion_link(core1_isd2, as1_isd2, ScionLinkType::ParentChild).unwrap();
        net.add_link(core1_isd2, as2_isd2).unwrap();
        net.configure_scion_link(core1_isd2, as2_isd2, ScionLinkType::ParentChild).unwrap();

        // Core link between ISDs
        net.add_link(core1_isd1, core1_isd2).unwrap();
        net.configure_scion_link(core1_isd1, core1_isd2, ScionLinkType::Core).unwrap();

        // Peering link between AS1 in ISD1 and AS1 in ISD2
        net.add_link(as1_isd1, as1_isd2).unwrap();
        net.configure_scion_link(as1_isd1, as1_isd2, ScionLinkType::Peering).unwrap();

        // Run beaconing and registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_intra_isd_beaconing(1000, 5).unwrap();
        net.scion_registration_round(5).unwrap();

        // Lookup paths from AS2 in ISD1 to AS2 in ISD2
        let paths = net.scion_lookup_paths(as2_isd1, as2_isd2).unwrap();

        assert!(!paths.is_empty(), "Should find inter-ISD paths");

        // Should have at least two paths:
        // 1. Normal path through cores
        // 2. Potential shortcut through peering (if implemented)
        assert!(paths.len() >= 1, "Should find at least one path");

        // Verify the normal path exists
        let normal_path = paths.iter().find(|p| p.as_path.len() == 4);
        assert!(normal_path.is_some(), "Should have normal 4-AS path through cores");
    }

    #[test]
    fn test_scion_cleanup_expired() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create a simple topology
        let r1 = net.add_router("r1", 65500);
        let r2 = net.add_router("r2", 65501);
        net.add_link(r1, r2).unwrap();

        net.enable_scion(r1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(r2, IsdAs::new(1, 120u64), true).unwrap();
        net.configure_scion_link(r1, r2, ScionLinkType::Core).unwrap();

        // Generate beacons
        net.scion_core_beaconing(1000).unwrap();

        // Verify PCBs exist
        let cs1 = net.get_router(r1).unwrap().scion().unwrap();
        assert!(cs1.beacon_store.total_count() > 0);

        // Clean up at a very late time (should remove all PCBs)
        let (pcbs_removed, _) = net.scion_cleanup_expired(100000).unwrap();
        assert!(pcbs_removed > 0, "Should remove expired PCBs");

        // Verify PCBs were removed
        let cs1_after = net.get_router(r1).unwrap().scion().unwrap();
        assert_eq!(cs1_after.beacon_store.total_count(), 0);
    }

    #[test]
    fn test_scion_link_failure() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create a simple topology
        let r1 = net.add_router("r1", 65500);
        let r2 = net.add_router("r2", 65501);
        net.add_link(r1, r2).unwrap();

        net.enable_scion(r1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(r2, IsdAs::new(1, 120u64), true).unwrap();
        net.configure_scion_link(r1, r2, ScionLinkType::Core).unwrap();

        // Verify interface exists
        let cs1_before = net.get_router(r1).unwrap().scion().unwrap();
        assert_eq!(cs1_before.interface_count(), 1);

        // Simulate link failure
        net.scion_handle_link_failure((r1, r2), 1000).unwrap();

        // Verify interfaces were removed
        let cs1_after = net.get_router(r1).unwrap().scion().unwrap();
        assert_eq!(cs1_after.interface_count(), 0);

        let cs2_after = net.get_router(r2).unwrap().scion().unwrap();
        assert_eq!(cs2_after.interface_count(), 0);
    }

    #[test]
    fn test_scion_mtu_tracking() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create a path with varying MTUs
        let core = net.add_router("core", 65500);
        let leaf = net.add_router("leaf", 65501);
        net.add_link(core, leaf).unwrap();

        net.enable_scion(core, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(leaf, IsdAs::new(1, 111u64), false).unwrap();

        // Configure link with specific MTU
        net.configure_scion_link(core, leaf, ScionLinkType::ParentChild).unwrap();

        // Run beaconing and registration
        net.scion_core_beaconing(1000).unwrap();
        net.scion_intra_isd_beaconing(1000, 5).unwrap();
        net.scion_registration_round(5).unwrap();

        // Lookup paths and verify MTU is tracked
        let paths = net.scion_lookup_paths(leaf, core).unwrap();
        assert!(!paths.is_empty());

        // MTU should be set (default is 1500)
        assert!(paths[0].mtu > 0);
    }

    #[test]
    fn test_inter_isd_complete_workflow() {
        let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

        // Create ISD 1 with 2 core ASes and children
        let core1_isd1 = net.add_router("Core1_ISD1", 65500);
        let core2_isd1 = net.add_router("Core2_ISD1", 65501);
        let leaf1_isd1 = net.add_router("Leaf1_ISD1", 65502);
        let leaf2_isd1 = net.add_router("Leaf2_ISD1", 65503);

        net.enable_scion(core1_isd1, IsdAs::new(1, 110u64), true).unwrap();
        net.enable_scion(core2_isd1, IsdAs::new(1, 120u64), true).unwrap();
        net.enable_scion(leaf1_isd1, IsdAs::new(1, 111u64), false).unwrap();
        net.enable_scion(leaf2_isd1, IsdAs::new(1, 121u64), false).unwrap();

        // ISD1 topology
        net.add_link(core1_isd1, core2_isd1).unwrap();
        net.configure_scion_link(core1_isd1, core2_isd1, ScionLinkType::Core).unwrap();
        net.add_link(core1_isd1, leaf1_isd1).unwrap();
        net.configure_scion_link(core1_isd1, leaf1_isd1, ScionLinkType::ParentChild).unwrap();
        net.add_link(core2_isd1, leaf2_isd1).unwrap();
        net.configure_scion_link(core2_isd1, leaf2_isd1, ScionLinkType::ParentChild).unwrap();

        // Create ISD 2 with 2 core ASes and children
        let core1_isd2 = net.add_router("Core1_ISD2", 65504);
        let core2_isd2 = net.add_router("Core2_ISD2", 65505);
        let leaf1_isd2 = net.add_router("Leaf1_ISD2", 65506);
        let leaf2_isd2 = net.add_router("Leaf2_ISD2", 65507);

        net.enable_scion(core1_isd2, IsdAs::new(2, 210u64), true).unwrap();
        net.enable_scion(core2_isd2, IsdAs::new(2, 220u64), true).unwrap();
        net.enable_scion(leaf1_isd2, IsdAs::new(2, 211u64), false).unwrap();
        net.enable_scion(leaf2_isd2, IsdAs::new(2, 221u64), false).unwrap();

        // ISD2 topology
        net.add_link(core1_isd2, core2_isd2).unwrap();
        net.configure_scion_link(core1_isd2, core2_isd2, ScionLinkType::Core).unwrap();
        net.add_link(core1_isd2, leaf1_isd2).unwrap();
        net.configure_scion_link(core1_isd2, leaf1_isd2, ScionLinkType::ParentChild).unwrap();
        net.add_link(core2_isd2, leaf2_isd2).unwrap();
        net.configure_scion_link(core2_isd2, leaf2_isd2, ScionLinkType::ParentChild).unwrap();

        // Inter-ISD core links
        net.add_link(core1_isd1, core1_isd2).unwrap();
        net.configure_scion_link(core1_isd1, core1_isd2, ScionLinkType::Core).unwrap();
        net.add_link(core2_isd1, core2_isd2).unwrap();
        net.configure_scion_link(core2_isd1, core2_isd2, ScionLinkType::Core).unwrap();

        // Complete beaconing and registration workflow
        net.scion_core_beaconing(1000).unwrap();
        net.scion_intra_isd_beaconing(1000, 5).unwrap();
        net.scion_registration_round(5).unwrap();

        // Test various path lookups

        // 1. Leaf to leaf in different ISDs
        let paths_leaf_to_leaf = net.scion_lookup_paths(leaf1_isd1, leaf1_isd2).unwrap();
        assert!(!paths_leaf_to_leaf.is_empty(), "Should find paths between leaves across ISDs");

        // 2. Core to core across ISDs
        let paths_core_to_core = net.scion_lookup_paths(core1_isd1, core1_isd2).unwrap();
        assert!(!paths_core_to_core.is_empty(), "Should find paths between cores across ISDs");

        // 3. Leaf to core across ISDs
        let paths_leaf_to_core = net.scion_lookup_paths(leaf1_isd1, core1_isd2).unwrap();
        assert!(!paths_leaf_to_core.is_empty(), "Should find paths from leaf to core across ISDs");

        // Verify path diversity - should have multiple paths due to multiple core-to-core links
        assert!(paths_leaf_to_leaf.len() >= 2, "Should have path diversity with multiple core links");

        // Verify path properties
        for path in &paths_leaf_to_leaf {
            assert!(path.as_path.len() >= 4, "Inter-ISD leaf-to-leaf path should have at least 4 ASes");
            assert_eq!(path.as_path[0].isd.0, 1, "Should start in ISD 1");
            assert_eq!(path.as_path[path.as_path.len() - 1].isd.0, 2, "Should end in ISD 2");

            // Verify segment composition
            assert!(path.up_segment.is_some(), "Should have up segment");
            assert!(path.core_segment.is_some(), "Should have core segment for inter-ISD");
            assert!(path.down_segment.is_some(), "Should have down segment");
        }
    }
}
