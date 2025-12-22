// Core SCION type definitions

use crate::types::ASN;
use serde::{Deserialize, Serialize};
use std::fmt;

/// ISD (Isolation Domain) number
///
/// ISDs group ASes for independent routing and trust management.
/// ISD numbers are 16-bit identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IsdNumber(pub u16);

impl IsdNumber {
    /// Create a new ISD number
    pub fn new(isd: u16) -> Self {
        Self(isd)
    }

    /// Get the ISD number as u16
    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

impl fmt::Display for IsdNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Combined ISD-AS identifier
///
/// In SCION, each AS is uniquely identified by the combination of its ISD number
/// and AS number. This struct represents that combined identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IsdAs {
    /// ISD number
    pub isd: IsdNumber,
    /// AS number
    pub asn: ASN,
}

impl IsdAs {
    /// Create a new ISD-AS identifier from raw values
    pub fn new(isd: u16, asn: u32) -> Self {
        Self {
            isd: IsdNumber(isd),
            asn: ASN(asn),
        }
    }

    /// Create an ISD-AS identifier from existing ISD and ASN types
    pub fn from_parts(isd: IsdNumber, asn: ASN) -> Self {
        Self { isd, asn }
    }
}

impl fmt::Display for IsdAs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.isd.0, self.asn.0)
    }
}

/// SCION interface identifier
///
/// Each AS-to-AS link is identified by a 16-bit interface ID local to the AS.
/// Interface IDs are used in hop fields to specify ingress/egress interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InterfaceId(pub u16);

impl InterfaceId {
    /// Create a new interface ID
    pub fn new(id: u16) -> Self {
        Self(id)
    }

    /// Get the interface ID as u16
    pub fn as_u16(&self) -> u16 {
        self.0
    }

    /// Special value indicating no interface (used for terminated segments)
    pub const ZERO: InterfaceId = InterfaceId(0);
}

impl fmt::Display for InterfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Type of link between two ASes
///
/// SCION distinguishes different link types that determine how PCBs are propagated
/// and how path segments are constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScionLinkType {
    /// Core-to-core link (between core ASes, possibly in different ISDs)
    Core,

    /// Parent link (from non-core to core, or to provider AS)
    Parent,

    /// Child link (from core or provider to customer AS)
    Child,

    /// Peering link (horizontal relationship, not traversed by PCBs directly)
    Peer,
}

impl ScionLinkType {
    /// Get the reverse link type (for bidirectional links)
    pub fn reverse(&self) -> Self {
        match self {
            Self::Core => Self::Core,
            Self::Parent => Self::Child,
            Self::Child => Self::Parent,
            Self::Peer => Self::Peer,
        }
    }

    /// Check if this is a core link
    pub fn is_core(&self) -> bool {
        matches!(self, Self::Core)
    }

    /// Check if this is a parent link
    pub fn is_parent(&self) -> bool {
        matches!(self, Self::Parent)
    }

    /// Check if this is a child link
    pub fn is_child(&self) -> bool {
        matches!(self, Self::Child)
    }

    /// Check if this is a peering link
    pub fn is_peer(&self) -> bool {
        matches!(self, Self::Peer)
    }
}

impl fmt::Display for ScionLinkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core => write!(f, "Core"),
            Self::Parent => write!(f, "Parent"),
            Self::Child => write!(f, "Child"),
            Self::Peer => write!(f, "Peer"),
        }
    }
}

/// SCION link metadata
///
/// Represents a link between two ASes with SCION-specific information.
#[derive(Debug, Clone)]
pub struct ScionLink {
    /// Type of link (core, parent, child, peer)
    pub link_type: ScionLinkType,
    /// Local interface ID
    pub local_interface: InterfaceId,
    /// Remote interface ID
    pub remote_interface: InterfaceId,
    /// Maximum transmission unit
    pub mtu: u16,
}

impl ScionLink {
    /// Create a new SCION link with default MTU (1500)
    pub fn new(
        link_type: ScionLinkType,
        local_interface: InterfaceId,
        remote_interface: InterfaceId,
    ) -> Self {
        Self {
            link_type,
            local_interface,
            remote_interface,
            mtu: 1500, // Default MTU
        }
    }

    /// Set a custom MTU for this link
    pub fn with_mtu(mut self, mtu: u16) -> Self {
        self.mtu = mtu;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_isd_number() {
        let isd = IsdNumber::new(1);
        assert_eq!(isd.as_u16(), 1);
        assert_eq!(isd.to_string(), "1");
    }

    #[test]
    fn test_isd_as() {
        let isd_as = IsdAs::new(1, 100);
        assert_eq!(isd_as.isd.as_u16(), 1);
        assert_eq!(isd_as.asn.0, 100);
        assert_eq!(isd_as.to_string(), "1-100");
    }

    #[test]
    fn test_isd_as_ordering() {
        let a = IsdAs::new(1, 100);
        let b = IsdAs::new(1, 200);
        let c = IsdAs::new(2, 100);

        assert!(a < b);
        assert!(a < c);
        assert!(b < c);
    }

    #[test]
    fn test_interface_id() {
        let iface = InterfaceId::new(42);
        assert_eq!(iface.as_u16(), 42);
        assert_eq!(iface.to_string(), "42");

        let zero = InterfaceId::ZERO;
        assert_eq!(zero.as_u16(), 0);
    }

    #[test]
    fn test_link_type_reverse() {
        assert_eq!(ScionLinkType::Core.reverse(), ScionLinkType::Core);
        assert_eq!(ScionLinkType::Parent.reverse(), ScionLinkType::Child);
        assert_eq!(ScionLinkType::Child.reverse(), ScionLinkType::Parent);
        assert_eq!(ScionLinkType::Peer.reverse(), ScionLinkType::Peer);
    }

    #[test]
    fn test_link_type_checks() {
        assert!(ScionLinkType::Core.is_core());
        assert!(!ScionLinkType::Parent.is_core());

        assert!(ScionLinkType::Parent.is_parent());
        assert!(!ScionLinkType::Core.is_parent());

        assert!(ScionLinkType::Child.is_child());
        assert!(!ScionLinkType::Peer.is_child());

        assert!(ScionLinkType::Peer.is_peer());
        assert!(!ScionLinkType::Child.is_peer());
    }

    #[test]
    fn test_scion_link() {
        let link = ScionLink::new(
            ScionLinkType::Parent,
            InterfaceId::new(1),
            InterfaceId::new(2),
        );

        assert_eq!(link.link_type, ScionLinkType::Parent);
        assert_eq!(link.local_interface.as_u16(), 1);
        assert_eq!(link.remote_interface.as_u16(), 2);
        assert_eq!(link.mtu, 1500);

        let link_custom_mtu = link.with_mtu(9000);
        assert_eq!(link_custom_mtu.mtu, 9000);
    }
}
