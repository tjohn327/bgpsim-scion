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

                // Select PCBs to propagate (convert Vec<&Pcb> to Vec<Pcb> first)
                let all_pcbs_owned: Vec<Pcb<P>> = all_pcbs.iter().map(|p| (*p).clone()).collect();
                let selected_pcbs = select_for_propagation(&all_pcbs_owned, &policy, max_propagate);

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
                // Only core ASes register down-segments
                if !cs.is_core {
                    continue;
                }

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
        let mut total_registered = 0;

        let router_ids: Vec<RouterId> = self.routers.keys().copied().collect();
        for router_id in router_ids {
            let router = self.get_router_mut(router_id)?;

            if let Some(cs) = router.scion_mut() {
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

                // Convert each selected PCB to a core-segment
                for pcb in selected_pcbs {
                    let core_segment = PathSegment::from_pcb(&pcb, SegmentType::Core);
                    cs.register_core_segment(core_segment).ok();
                    total_registered += 1;
                }
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

    /// Lookup paths from source to destination (intra-ISD).
    ///
    /// For paths within the same ISD, this combines up-segments and down-segments.
    ///
    /// # Arguments
    /// * `src` - Source router
    /// * `dst` - Destination router
    ///
    /// # Returns
    /// `Ok(Vec<ForwardingPath>)` - Vector of available paths
    pub fn scion_lookup_intra_isd_paths(
        &self,
        src: RouterId,
        dst: RouterId,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        use crate::scion::ForwardingPath;

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
            let core = self.get_router(core_router)?;
            let core_cs = core.scion().unwrap();
            let core_isd_as = core_cs.isd_as;

            // Get up-segment from src to core
            let up_segments = if src_cs.is_core {
                vec![] // Source is already core, no up-segment needed
            } else {
                // Look for up-segments TO this core FROM the source
                core_cs.lookup_up_segments(&core_isd_as)
                    .into_iter()
                    .filter(|seg| seg.source() == Some(src_isd_as))
                    .map(|seg| seg.clone())
                    .collect()
            };

            // Get down-segment from core to dst
            let down_segments = if dst_cs.is_core {
                vec![] // Destination is core, no down-segment needed
            } else {
                // Look for down-segments TO the destination FROM this core
                core_cs.path_database.lookup_down_segments_to(&dst_isd_as)
                    .into_iter()
                    .filter(|seg| seg.source() == Some(core_isd_as))
                    .map(|seg| seg.clone())
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
                    for down_seg in &down_segments {
                        if let Ok(path) = ForwardingPath::new(Some(up_seg.clone()), None, Some(down_seg.clone())) {
                            paths.push(path);
                        }
                    }
                }
            }
        }

        Ok(paths)
    }

    /// Lookup paths from source to destination (inter-ISD).
    ///
    /// For paths across ISDs, this combines up-segments, core-segments, and down-segments.
    ///
    /// # Arguments
    /// * `src` - Source router
    /// * `dst` - Destination router
    ///
    /// # Returns
    /// `Ok(Vec<ForwardingPath>)` - Vector of available paths
    pub fn scion_lookup_inter_isd_paths(
        &self,
        src: RouterId,
        dst: RouterId,
    ) -> Result<Vec<crate::scion::ForwardingPath<P>>, NetworkError> {
        use crate::scion::ForwardingPath;

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
            for dst_core_router in &dst_cores {
                let src_core = self.get_router(*src_core_router)?;
                let dst_core = self.get_router(*dst_core_router)?;

                let src_core_cs = src_core.scion().unwrap();
                let dst_core_cs = dst_core.scion().unwrap();

                let src_core_isd_as = src_core_cs.isd_as;
                let dst_core_isd_as = dst_core_cs.isd_as;

                // Get up-segment from src to src_core
                let up_segments = if src_cs.is_core {
                    vec![]
                } else {
                    src_core_cs.lookup_up_segments(&src_isd_as)
                };

                // Get core-segment from src_core to dst_core
                let core_segments = src_core_cs.lookup_core_segments(&src_core_isd_as, &dst_core_isd_as);

                // Get down-segment from dst_core to dst
                let down_segments = if dst_cs.is_core {
                    vec![]
                } else {
                    dst_core_cs.lookup_down_segments(&dst_isd_as)
                };

                // Combine segments
                for core_seg in &core_segments {
                    if src_cs.is_core && dst_cs.is_core {
                        // Core to core across ISDs: only core-segment
                        if let Ok(path) = ForwardingPath::new(None, Some(core_seg.clone()), None) {
                            paths.push(path);
                        }
                    } else if src_cs.is_core && !dst_cs.is_core {
                        // Core to non-core across ISDs: core + down
                        for down_seg in &down_segments {
                            if let Ok(path) = ForwardingPath::new(None, Some(core_seg.clone()), Some(down_seg.clone())) {
                                paths.push(path);
                            }
                        }
                    } else if !src_cs.is_core && dst_cs.is_core {
                        // Non-core to core across ISDs: up + core
                        for up_seg in &up_segments {
                            if let Ok(path) = ForwardingPath::new(Some(up_seg.clone()), Some(core_seg.clone()), None) {
                                paths.push(path);
                            }
                        }
                    } else {
                        // Non-core to non-core across ISDs: up + core + down
                        for up_seg in &up_segments {
                            for down_seg in &down_segments {
                                if let Ok(path) = ForwardingPath::new(Some(up_seg.clone()), Some(core_seg.clone()), Some(down_seg.clone())) {
                                    paths.push(path);
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
    /// # Arguments
    /// * `src` - Source router
    /// * `dst` - Destination router
    ///
    /// # Returns
    /// `Ok(Vec<ForwardingPath>)` - Vector of available paths
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
        assert_eq!(registered, 2); // core1 and core2 each register 1 core-segment

        // Verify core-segments were registered
        let core1_router = net.get_router(core1).unwrap();
        let core1_cs = core1_router.scion().unwrap();
        let core_segs = core1_cs.path_database.get_all_core_segments();
        assert_eq!(core_segs.len(), 1); // core1 registered core2's PCB
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
        assert_eq!(core_count, 2); // core1 and core2 each register 1 core-segment
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
}
