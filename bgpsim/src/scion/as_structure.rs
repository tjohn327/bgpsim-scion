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

//! SCION AS-level structure supporting multiple border routers per AS.
//!
//! This module implements the realistic SCION architecture where:
//! - One AS can have multiple border routers
//! - Each border router owns specific external interfaces
//! - The Control Service is shared across all routers in an AS

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::types::{IntoIpv4Prefix, Ipv4Prefix, Prefix, RouterId};

use super::{
    process::ScionControlService,
    types::{InterfaceId, IsdAs},
    ScionSimulationMode,
};

/// Represents a SCION Autonomous System with support for multiple border routers.
///
/// In real SCION, an AS is a logical entity that can have multiple border routers,
/// each handling different external links. This structure captures that architecture:
///
/// - **One Control Service per AS** (shared across all routers)
/// - **Multiple border routers** (each owns specific interfaces)
/// - **Interface ownership tracking** (maps interfaces to specific routers)
///
/// # Example
///
/// ```text
/// AS 1-110 (Core AS):
///   ├── Border Router BR1 owns interfaces [1, 2]
///   │   ├── Interface 1 → AS 1-111
///   │   └── Interface 2 → AS 1-112
///   └── Border Router BR2 owns interface [3]
///       └── Interface 3 → AS 2-210
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "P: Prefix + Serialize + for<'d> Deserialize<'d>")]
pub struct ScionAs<P: Prefix> {
    /// The ISD-AS identifier for this AS
    pub isd_as: IsdAs,

    /// Whether this AS is a core AS (participates in core beaconing)
    pub is_core: bool,

    /// The SCION Control Service for this AS (shared across all routers)
    pub control_service: ScionControlService<P>,

    /// Current simulation mode for this AS.
    simulation_mode: ScionSimulationMode,

    /// Set of border routers belonging to this AS
    ///
    /// Border routers are routers that have external SCION links.
    /// All routers in `border_routers` handle SCION control plane operations.
    border_routers: HashSet<RouterId>,

    /// Maps SCION interface IDs to the router that owns them
    ///
    /// When a PCB arrives on an interface, this map determines which
    /// router should handle the incoming beacon.
    interface_owners: HashMap<InterfaceId, RouterId>,

    /// Reverse mapping: router → list of interfaces it owns
    ///
    /// Used for efficient lookup when propagating beacons from a router.
    router_interfaces: HashMap<RouterId, Vec<InterfaceId>>,
}

impl<P: Prefix> ScionAs<P> {
    /// Create a new SCION AS with no border routers.
    ///
    /// # Arguments
    /// * `isd_as` - The ISD-AS identifier for this AS
    /// * `is_core` - Whether this is a core AS
    ///
    /// # Returns
    /// A new `ScionAs` with an empty Control Service and no border routers.
    ///
    /// # Example
    /// ```
    /// use bgpsim::scion::{ScionAs, ScionSimulationMode};
    /// use bgpsim::scion::IsdAs;
    /// use bgpsim::types::SimplePrefix;
    ///
    /// let scion_as: ScionAs<SimplePrefix> =
    ///     ScionAs::new(IsdAs::new(1, 110), true, ScionSimulationMode::Dynamic);
    /// assert_eq!(scion_as.isd_as, IsdAs::new(1, 110));
    /// assert!(scion_as.is_core);
    /// assert_eq!(scion_as.border_router_count(), 0);
    /// ```
    pub fn new(isd_as: IsdAs, is_core: bool, mode: ScionSimulationMode) -> Self {
        ScionAs {
            isd_as,
            is_core,
            control_service: ScionControlService::with_mode(isd_as, is_core, mode),
            simulation_mode: mode,
            border_routers: HashSet::new(),
            interface_owners: HashMap::new(),
            router_interfaces: HashMap::new(),
        }
    }

    /// Return the simulation mode configured for this AS.
    pub fn simulation_mode(&self) -> ScionSimulationMode {
        self.simulation_mode
    }

    /// Update the simulation mode and propagate changes to the control service.
    pub fn set_simulation_mode(&mut self, mode: ScionSimulationMode) {
        if self.simulation_mode != mode {
            self.simulation_mode = mode;
            self.control_service.set_simulation_mode(mode);
        }
    }

    /// Add a border router to this AS.
    ///
    /// # Arguments
    /// * `router` - The router ID to add as a border router
    ///
    /// # Returns
    /// `true` if the router was newly added, `false` if it was already present
    ///
    /// # Example
    /// ```
    /// # use bgpsim::scion::ScionAs;
    /// # use bgpsim::scion::IsdAs;
    /// # use bgpsim::types::{SimplePrefix, RouterId};
    /// let mut scion_as: ScionAs<SimplePrefix> =
    ///     ScionAs::new(IsdAs::new(1, 110), true, ScionSimulationMode::Dynamic);
    /// let br1 = RouterId::from(1u32);
    ///
    /// assert!(scion_as.add_border_router(br1));  // Newly added
    /// assert!(!scion_as.add_border_router(br1)); // Already present
    /// ```
    pub fn add_border_router(&mut self, router: RouterId) -> bool {
        self.border_routers.insert(router)
    }

    /// Remove a border router from this AS.
    ///
    /// Also removes all interface ownership records for this router.
    ///
    /// # Arguments
    /// * `router` - The router ID to remove
    ///
    /// # Returns
    /// `true` if the router was present and removed, `false` if not found
    pub fn remove_border_router(&mut self, router: RouterId) -> bool {
        // Remove from border routers set
        let removed = self.border_routers.remove(&router);

        if removed {
            // Clean up interface ownership
            if let Some(interfaces) = self.router_interfaces.remove(&router) {
                for interface_id in interfaces {
                    self.interface_owners.remove(&interface_id);
                }
            }
        }

        removed
    }

    /// Check if a router is a border router of this AS.
    ///
    /// # Arguments
    /// * `router` - The router ID to check
    ///
    /// # Returns
    /// `true` if the router is a border router of this AS
    pub fn is_border_router(&self, router: RouterId) -> bool {
        self.border_routers.contains(&router)
    }

    /// Get the number of border routers in this AS.
    ///
    /// # Returns
    /// The count of border routers
    pub fn border_router_count(&self) -> usize {
        self.border_routers.len()
    }

    /// Get an iterator over all border routers.
    ///
    /// # Returns
    /// An iterator over router IDs
    pub fn border_routers_iter(&self) -> impl Iterator<Item = &RouterId> {
        self.border_routers.iter()
    }

    /// Assign an interface to a specific router.
    ///
    /// This establishes that the given router owns the specified interface.
    /// When beacons arrive on this interface, they will be handled by this router.
    ///
    /// # Arguments
    /// * `interface_id` - The SCION interface ID
    /// * `router` - The router that owns this interface
    ///
    /// # Panics
    /// Panics if the router is not a border router of this AS (debug builds only)
    ///
    /// # Example
    /// ```
    /// # use bgpsim::scion::ScionAs;
    /// # use bgpsim::scion::{IsdAs, InterfaceId};
    /// # use bgpsim::types::{SimplePrefix, RouterId};
    /// let mut scion_as: ScionAs<SimplePrefix> =
    ///     ScionAs::new(IsdAs::new(1, 110), true, ScionSimulationMode::Dynamic);
    /// let br1 = RouterId::from(1u32);
    /// scion_as.add_border_router(br1);
    ///
    /// scion_as.assign_interface(InterfaceId(1), br1);
    /// assert_eq!(scion_as.get_interface_owner(InterfaceId(1)), Some(br1));
    /// ```
    pub fn assign_interface(&mut self, interface_id: InterfaceId, router: RouterId) {
        debug_assert!(
            self.border_routers.contains(&router),
            "Router {:?} is not a border router of AS {}",
            router,
            self.isd_as
        );

        self.interface_owners.insert(interface_id, router);
        self.router_interfaces
            .entry(router)
            .or_default()
            .push(interface_id);
    }

    /// Get the router that owns a specific interface.
    ///
    /// # Arguments
    /// * `interface_id` - The SCION interface ID
    ///
    /// # Returns
    /// `Some(router)` if the interface is owned by a router, `None` otherwise
    pub fn get_interface_owner(&self, interface_id: InterfaceId) -> Option<RouterId> {
        self.interface_owners.get(&interface_id).copied()
    }

    /// Get all interfaces owned by a specific router.
    ///
    /// # Arguments
    /// * `router` - The router ID
    ///
    /// # Returns
    /// A slice of interface IDs owned by this router (empty if router has no interfaces)
    pub fn get_router_interfaces(&self, router: RouterId) -> &[InterfaceId] {
        self.router_interfaces
            .get(&router)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get the total number of interfaces in this AS.
    ///
    /// # Returns
    /// The count of interfaces
    pub fn interface_count(&self) -> usize {
        self.control_service.interface_count()
    }

    /// Return the set of neighboring core ASes connected via core links.
    pub fn core_neighbors(&self) -> Vec<IsdAs> {
        use std::collections::HashSet;
        let mut neighbors = HashSet::new();
        for iface in self.control_service.get_core_interfaces() {
            neighbors.insert(iface.neighbor_isd_as);
        }
        neighbors.into_iter().collect()
    }

    /// Access the Control Service for this AS.
    ///
    /// # Returns
    /// A reference to the SCION Control Service
    pub fn control_service(&self) -> &ScionControlService<P> {
        &self.control_service
    }

    /// Access the Control Service mutably.
    ///
    /// # Returns
    /// A mutable reference to the SCION Control Service
    pub fn control_service_mut(&mut self) -> &mut ScionControlService<P> {
        &mut self.control_service
    }

    /// Add an interface to the AS and assign it to a specific border router.
    ///
    /// This is a convenience method that combines interface creation with ownership assignment.
    ///
    /// # Arguments
    /// * `interface_info` - Information about the interface to add
    /// * `owner_router` - The router that owns this interface
    ///
    /// # Returns
    /// `Ok(())` if successful, `Err(String)` if the interface ID is already in use
    ///
    /// # Example
    /// ```ignore
    /// let info = InterfaceInfo::new(InterfaceId(1), neighbor_isd_as, link_type, neighbor_router, 1500);
    /// scion_as.add_interface_for_router(info, br1)?;
    /// ```
    pub fn add_interface_for_router(
        &mut self,
        interface_info: super::types::InterfaceInfo,
        owner_router: RouterId,
    ) -> Result<(), String> {
        // Verify the router is a border router of this AS
        if !self.border_routers.contains(&owner_router) {
            return Err(format!(
                "Router {:?} is not a border router of AS {}",
                owner_router, self.isd_as
            ));
        }

        let interface_id = interface_info.interface_id;

        // Add interface to the AS's control service
        self.control_service.add_interface(interface_info)?;

        // Track ownership
        self.assign_interface(interface_id, owner_router);

        Ok(())
    }

    /// Get the next available interface ID for this AS.
    ///
    /// # Returns
    /// The next interface ID to use
    pub fn next_interface_id(&mut self) -> InterfaceId {
        self.control_service.next_interface_id()
    }

    /// Convert to use Ipv4Prefix (for network conversion)
    pub fn into_ipv4_prefix(self) -> ScionAs<Ipv4Prefix>
    where
        P: Prefix,
    {
        ScionAs {
            isd_as: self.isd_as,
            is_core: self.is_core,
            control_service: self.control_service.into_ipv4_prefix(),
            simulation_mode: self.simulation_mode,
            border_routers: self.border_routers,
            interface_owners: self.interface_owners,
            router_interfaces: self.router_interfaces,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SimplePrefix;

    #[test]
    fn test_new_scion_as() {
        let isd_as = IsdAs::new(1, 110u64);
        let scion_as: ScionAs<SimplePrefix> =
            ScionAs::new(isd_as, true, ScionSimulationMode::Dynamic);

        assert_eq!(scion_as.isd_as, isd_as);
        assert!(scion_as.is_core);
        assert_eq!(scion_as.border_router_count(), 0);
        assert_eq!(scion_as.interface_count(), 0);
    }

    #[test]
    fn test_add_border_router() {
        let mut scion_as: ScionAs<SimplePrefix> =
            ScionAs::new(IsdAs::new(1, 110u64), false, ScionSimulationMode::Dynamic);
        let br1 = RouterId::from(1u32);
        let br2 = RouterId::from(2u32);

        assert!(scion_as.add_border_router(br1));
        assert_eq!(scion_as.border_router_count(), 1);
        assert!(scion_as.is_border_router(br1));

        assert!(scion_as.add_border_router(br2));
        assert_eq!(scion_as.border_router_count(), 2);

        // Adding same router again should return false
        assert!(!scion_as.add_border_router(br1));
        assert_eq!(scion_as.border_router_count(), 2);
    }

    #[test]
    fn test_remove_border_router() {
        let mut scion_as: ScionAs<SimplePrefix> =
            ScionAs::new(IsdAs::new(1, 110u64), false, ScionSimulationMode::Dynamic);
        let br1 = RouterId::from(1u32);

        scion_as.add_border_router(br1);
        assert!(scion_as.is_border_router(br1));

        assert!(scion_as.remove_border_router(br1));
        assert!(!scion_as.is_border_router(br1));
        assert_eq!(scion_as.border_router_count(), 0);

        // Removing again should return false
        assert!(!scion_as.remove_border_router(br1));
    }

    #[test]
    fn test_assign_interface() {
        let mut scion_as: ScionAs<SimplePrefix> =
            ScionAs::new(IsdAs::new(1, 110u64), false, ScionSimulationMode::Dynamic);
        let br1 = RouterId::from(1u32);
        scion_as.add_border_router(br1);

        let if1 = InterfaceId(1);
        let if2 = InterfaceId(2);

        scion_as.assign_interface(if1, br1);
        scion_as.assign_interface(if2, br1);

        assert_eq!(scion_as.get_interface_owner(if1), Some(br1));
        assert_eq!(scion_as.get_interface_owner(if2), Some(br1));

        let interfaces = scion_as.get_router_interfaces(br1);
        assert_eq!(interfaces.len(), 2);
        assert!(interfaces.contains(&if1));
        assert!(interfaces.contains(&if2));
    }

    #[test]
    fn test_remove_router_cleans_interfaces() {
        let mut scion_as: ScionAs<SimplePrefix> =
            ScionAs::new(IsdAs::new(1, 110u64), false, ScionSimulationMode::Dynamic);
        let br1 = RouterId::from(1u32);
        scion_as.add_border_router(br1);

        let if1 = InterfaceId(1);
        scion_as.assign_interface(if1, br1);

        assert_eq!(scion_as.get_interface_owner(if1), Some(br1));

        // Remove router should clean up interfaces
        scion_as.remove_border_router(br1);
        assert_eq!(scion_as.get_interface_owner(if1), None);
        assert_eq!(scion_as.get_router_interfaces(br1).len(), 0);
    }

    #[test]
    fn test_multiple_routers_different_interfaces() {
        let mut scion_as: ScionAs<SimplePrefix> =
            ScionAs::new(IsdAs::new(1, 110u64), true, ScionSimulationMode::Dynamic);
        let br1 = RouterId::from(1u32);
        let br2 = RouterId::from(2u32);

        scion_as.add_border_router(br1);
        scion_as.add_border_router(br2);

        let if1 = InterfaceId(1);
        let if2 = InterfaceId(2);
        let if3 = InterfaceId(3);

        scion_as.assign_interface(if1, br1);
        scion_as.assign_interface(if2, br1);
        scion_as.assign_interface(if3, br2);

        // BR1 owns if1 and if2
        let br1_ifs = scion_as.get_router_interfaces(br1);
        assert_eq!(br1_ifs.len(), 2);
        assert!(br1_ifs.contains(&if1));
        assert!(br1_ifs.contains(&if2));

        // BR2 owns if3
        let br2_ifs = scion_as.get_router_interfaces(br2);
        assert_eq!(br2_ifs.len(), 1);
        assert!(br2_ifs.contains(&if3));

        // Check ownership
        assert_eq!(scion_as.get_interface_owner(if1), Some(br1));
        assert_eq!(scion_as.get_interface_owner(if2), Some(br1));
        assert_eq!(scion_as.get_interface_owner(if3), Some(br2));
    }

    #[test]
    fn test_border_routers_iter() {
        let mut scion_as: ScionAs<SimplePrefix> =
            ScionAs::new(IsdAs::new(1, 110u64), false, ScionSimulationMode::Dynamic);
        let br1 = RouterId::from(1u32);
        let br2 = RouterId::from(2u32);
        let br3 = RouterId::from(3u32);

        scion_as.add_border_router(br1);
        scion_as.add_border_router(br2);
        scion_as.add_border_router(br3);

        let routers: Vec<_> = scion_as.border_routers_iter().copied().collect();
        assert_eq!(routers.len(), 3);
        assert!(routers.contains(&br1));
        assert!(routers.contains(&br2));
        assert!(routers.contains(&br3));
    }
}
