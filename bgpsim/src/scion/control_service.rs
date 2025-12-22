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
