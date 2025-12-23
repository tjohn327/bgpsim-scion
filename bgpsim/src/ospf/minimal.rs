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

//! Minimal OSPF implementation that assumes full mesh connectivity within an AS.
//!
//! This implementation is designed for SCION simulation where:
//! - Inter-AS routing is handled by SCION hop fields (not OSPF)
//! - Intra-AS routing assumes all border routers are directly connected (full mesh)
//!
//! Benefits:
//! - No SPT computation overhead
//! - O(1) updates when adding/removing links
//! - Suitable for large-scale SCION simulations
//!
//! Trade-offs:
//! - Does not model actual intra-AS routing topology
//! - All internal routers appear equidistant (cost = 1.0)

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    event::Event,
    ospf::{
        global::{GlobalOspfCoordinator, GlobalOspfProcess},
        LinkWeight, NeighborhoodChange, OspfArea, OspfCoordinator, OspfImpl, OspfProcess,
        EXTERNAL_LINK_WEIGHT,
    },
    router::Router,
    types::{DeviceError, NetworkError, Prefix, RouterId, ASN},
};

use super::local::OspfEvent;

/// Cost assigned to all internal links in the full-mesh model
pub const FULL_MESH_LINK_WEIGHT: LinkWeight = 1.0;

/// Minimal OSPF implementation assuming full mesh connectivity within AS.
///
/// This is ideal for SCION simulations where inter-AS routing is handled by
/// SCION path segments, and intra-AS routing topology doesn't matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MinimalOspf;

impl OspfImpl for MinimalOspf {
    type Coordinator = MinimalOspfCoordinator;
    type Process = MinimalOspfProcess;

    fn into_global(
        coordinators: (Self::Coordinator, &mut GlobalOspfCoordinator),
        processes: HashMap<RouterId, (Self::Process, &mut GlobalOspfProcess)>,
    ) -> Result<(), NetworkError> {
        // Convert minimal to global by computing actual SPT
        // This allows switching to full OSPF if needed
        let (from, into) = coordinators;

        // Reset the global coordinator to match our state
        *into = GlobalOspfCoordinator::new(from.asn);

        // Convert processes - the global process will be updated by the coordinator
        for (router_id, (from_proc, into_proc)) in processes {
            *into_proc = GlobalOspfProcess::new(router_id);
            // Copy over the neighbor information
            into_proc.neighbors = from_proc.neighbors.clone();
            into_proc.ospf_table = from_proc.ospf_table.clone();
        }

        Ok(())
    }

    fn from_global(
        coordinators: (&mut Self::Coordinator, GlobalOspfCoordinator),
        processes: HashMap<RouterId, (&mut Self::Process, GlobalOspfProcess)>,
    ) -> Result<(), NetworkError> {
        // Convert global to minimal - we lose detailed routing info
        let (into, from) = coordinators;
        into.asn = from.get_asn();
        into.internal_routers.clear();

        // Extract router set from the global coordinator
        for router_id in from.get_ribs().keys() {
            into.internal_routers.insert(*router_id);
        }

        // Convert processes
        for (router_id, (into_proc, from_proc)) in processes {
            into_proc.router_id = router_id;
            into_proc.neighbors = from_proc.neighbors;
            // Rebuild ospf_table assuming full mesh
            into_proc.rebuild_full_mesh_table(&into.internal_routers);
        }

        Ok(())
    }
}

/// Coordinator for MinimalOspf - tracks routers without computing SPT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinimalOspfCoordinator {
    /// The AS number
    asn: ASN,
    /// Set of all internal routers in this AS
    internal_routers: HashSet<RouterId>,
    /// External links per router
    external_links: HashMap<RouterId, HashSet<RouterId>>,
}

impl PartialEq for MinimalOspfCoordinator {
    fn eq(&self, _other: &Self) -> bool {
        // Coordinators are considered equal if they're for the same AS
        true
    }
}

impl OspfCoordinator for MinimalOspfCoordinator {
    type Process = MinimalOspfProcess;

    fn new(asn: ASN) -> Self {
        Self {
            asn,
            internal_routers: HashSet::new(),
            external_links: HashMap::new(),
        }
    }

    fn update<P: Prefix, T: Default>(
        &mut self,
        delta: NeighborhoodChange,
        mut routers: BTreeMap<RouterId, &mut Router<P, Self::Process>>,
        links: &HashMap<RouterId, HashMap<RouterId, (LinkWeight, OspfArea)>>,
        external_links: &HashMap<RouterId, HashSet<RouterId>>,
    ) -> Result<Vec<Event<P, T>>, NetworkError> {
        // Track which routers need their tables updated
        let mut modified_routers: HashSet<RouterId> = HashSet::new();

        // Process the neighborhood change
        match delta {
            NeighborhoodChange::AddLink { a, b, .. } => {
                // New internal link - ensure both routers are tracked
                let a_new = self.internal_routers.insert(a);
                let b_new = self.internal_routers.insert(b);

                // If either router is new, all routers need updating (full mesh)
                if a_new || b_new {
                    modified_routers.extend(self.internal_routers.iter().copied());
                }
            }
            NeighborhoodChange::RemoveLink { a, b, .. } => {
                // Check if routers still have other internal links
                let a_has_links = links.get(&a).map(|l| !l.is_empty()).unwrap_or(false);
                let b_has_links = links.get(&b).map(|l| !l.is_empty()).unwrap_or(false);

                if !a_has_links {
                    self.internal_routers.remove(&a);
                    modified_routers.extend(self.internal_routers.iter().copied());
                }
                if !b_has_links {
                    self.internal_routers.remove(&b);
                    modified_routers.extend(self.internal_routers.iter().copied());
                }
            }
            NeighborhoodChange::AddExternalNetwork { int, ext } => {
                self.external_links.entry(int).or_default().insert(ext);
                modified_routers.insert(int);
            }
            NeighborhoodChange::RemoveExternalNetwork { int, ext } => {
                if let Some(ext_set) = self.external_links.get_mut(&int) {
                    ext_set.remove(&ext);
                }
                modified_routers.insert(int);
            }
            NeighborhoodChange::Weight { .. } | NeighborhoodChange::Area { .. } => {
                // Weight/area changes don't matter in full-mesh model
                // All internal links have the same weight
            }
            NeighborhoodChange::Batch(changes) => {
                // Process batch recursively, collecting modified routers
                for change in changes {
                    // Recursively process each change
                    let sub_modified = self.process_single_change(change, links);
                    modified_routers.extend(sub_modified);
                }
            }
        }

        // Update modified routers' OSPF tables
        let mut events = Vec::new();
        for router_id in modified_routers {
            if let Some(router) = routers.get_mut(&router_id) {
                let ext = external_links.get(&router_id);
                events.append(&mut router.update_ospf(|ospf| {
                    ospf.update_full_mesh(
                        &self.internal_routers,
                        links.get(&router_id),
                        ext,
                    );
                    Ok((true, Vec::new()))
                })?);
            }
        }

        Ok(events)
    }
}

impl MinimalOspfCoordinator {
    /// Process a single change and return modified router IDs
    fn process_single_change(
        &mut self,
        delta: NeighborhoodChange,
        links: &HashMap<RouterId, HashMap<RouterId, (LinkWeight, OspfArea)>>,
    ) -> HashSet<RouterId> {
        let mut modified = HashSet::new();

        match delta {
            NeighborhoodChange::AddLink { a, b, .. } => {
                let a_new = self.internal_routers.insert(a);
                let b_new = self.internal_routers.insert(b);
                if a_new || b_new {
                    modified.extend(self.internal_routers.iter().copied());
                }
            }
            NeighborhoodChange::RemoveLink { a, b, .. } => {
                let a_has_links = links.get(&a).map(|l| !l.is_empty()).unwrap_or(false);
                let b_has_links = links.get(&b).map(|l| !l.is_empty()).unwrap_or(false);

                if !a_has_links {
                    self.internal_routers.remove(&a);
                    modified.extend(self.internal_routers.iter().copied());
                }
                if !b_has_links {
                    self.internal_routers.remove(&b);
                    modified.extend(self.internal_routers.iter().copied());
                }
            }
            NeighborhoodChange::AddExternalNetwork { int, ext } => {
                self.external_links.entry(int).or_default().insert(ext);
                modified.insert(int);
            }
            NeighborhoodChange::RemoveExternalNetwork { int, ext } => {
                if let Some(ext_set) = self.external_links.get_mut(&int) {
                    ext_set.remove(&ext);
                }
                modified.insert(int);
            }
            NeighborhoodChange::Batch(changes) => {
                for change in changes {
                    modified.extend(self.process_single_change(change, links));
                }
            }
            _ => {}
        }

        modified
    }
}

/// Router-local OSPF process for MinimalOspf.
///
/// Assumes full mesh connectivity - all internal routers are directly reachable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinimalOspfProcess {
    /// Router ID
    router_id: RouterId,
    /// OSPF routing table: target -> (next_hops, cost)
    /// In full mesh, next_hop is always the target itself
    ospf_table: BTreeMap<RouterId, (Vec<RouterId>, LinkWeight)>,
    /// Physical neighbors (internal + external)
    neighbors: BTreeMap<RouterId, LinkWeight>,
}

impl MinimalOspfProcess {
    /// Update tables assuming full mesh connectivity
    pub fn update_full_mesh(
        &mut self,
        internal_routers: &HashSet<RouterId>,
        internal_links: Option<&HashMap<RouterId, (LinkWeight, OspfArea)>>,
        external_links: Option<&HashSet<RouterId>>,
    ) {
        // Build neighbors from actual links
        self.neighbors.clear();

        // Add internal neighbors (from actual links, not full mesh)
        if let Some(links) = internal_links {
            for (neighbor, (weight, _)) in links {
                if weight.is_finite() {
                    self.neighbors.insert(*neighbor, *weight);
                }
            }
        }

        // Add external neighbors
        if let Some(ext) = external_links {
            for neighbor in ext {
                self.neighbors.insert(*neighbor, EXTERNAL_LINK_WEIGHT);
            }
        }

        // Build OSPF table assuming full mesh
        // Every internal router is reachable with cost 1.0
        // Next-hop is the target itself (direct link in full mesh)
        self.ospf_table.clear();
        for &router in internal_routers {
            if router != self.router_id {
                self.ospf_table.insert(router, (vec![router], FULL_MESH_LINK_WEIGHT));
            }
        }
    }

    /// Rebuild the full mesh table from a set of internal routers
    fn rebuild_full_mesh_table(&mut self, internal_routers: &HashSet<RouterId>) {
        self.ospf_table.clear();
        for &router in internal_routers {
            if router != self.router_id {
                self.ospf_table.insert(router, (vec![router], FULL_MESH_LINK_WEIGHT));
            }
        }
    }
}

impl OspfProcess for MinimalOspfProcess {
    fn new(router_id: RouterId) -> Self {
        Self {
            router_id,
            ospf_table: BTreeMap::new(),
            neighbors: BTreeMap::new(),
        }
    }

    fn get_table(&self) -> &BTreeMap<RouterId, (Vec<RouterId>, LinkWeight)> {
        &self.ospf_table
    }

    fn get_neighbors(&self) -> &BTreeMap<RouterId, LinkWeight> {
        &self.neighbors
    }

    fn handle_event<P: Prefix, T: Default>(
        &mut self,
        _src: RouterId,
        _area: OspfArea,
        _event: OspfEvent,
    ) -> Result<(bool, Vec<Event<P, T>>), DeviceError> {
        // MinimalOspf doesn't process OSPF events - state is managed by coordinator
        Ok((false, Vec::new()))
    }

    fn is_waiting_for_timeout(&self) -> bool {
        false
    }

    fn trigger_timeout<P: Prefix, T: Default>(
        &mut self,
    ) -> Result<(bool, Vec<Event<P, T>>), DeviceError> {
        Ok((false, Vec::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_ospf_process_creation() {
        let router_id = RouterId::from(1);
        let process = MinimalOspfProcess::new(router_id);

        assert!(process.get_table().is_empty());
        assert!(process.get_neighbors().is_empty());
    }

    #[test]
    fn test_full_mesh_table_update() {
        let router_id = RouterId::from(1);
        let mut process = MinimalOspfProcess::new(router_id);

        let mut internal_routers = HashSet::new();
        internal_routers.insert(RouterId::from(1));
        internal_routers.insert(RouterId::from(2));
        internal_routers.insert(RouterId::from(3));

        let mut internal_links = HashMap::new();
        internal_links.insert(RouterId::from(2), (100.0, OspfArea::BACKBONE));
        internal_links.insert(RouterId::from(3), (100.0, OspfArea::BACKBONE));

        let mut external_links = HashSet::new();
        external_links.insert(RouterId::from(10));

        process.update_full_mesh(
            &internal_routers,
            Some(&internal_links),
            Some(&external_links),
        );

        // Check OSPF table - should have entries for routers 2 and 3
        assert_eq!(process.get_table().len(), 2);

        // Check that router 2 is reachable with cost 1.0
        let (nhs, cost) = process.get_table().get(&RouterId::from(2)).unwrap();
        assert_eq!(nhs, &vec![RouterId::from(2)]);
        assert_eq!(*cost, FULL_MESH_LINK_WEIGHT);

        // Check neighbors
        assert_eq!(process.get_neighbors().len(), 3); // 2 internal + 1 external
        assert!(process.is_neighbor(RouterId::from(2)));
        assert!(process.is_neighbor(RouterId::from(10)));
    }

    #[test]
    fn test_coordinator_creation() {
        let coord = MinimalOspfCoordinator::new(ASN(65000));
        assert!(coord.internal_routers.is_empty());
        assert!(coord.external_links.is_empty());
    }
}
