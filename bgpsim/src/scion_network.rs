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
                    let core_interfaces = cs.get_core_interfaces();

                    let mut pcbs_for_router = Vec::new();

                    // Create a PCB for each core neighbor
                    for interface in core_interfaces {
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
}
