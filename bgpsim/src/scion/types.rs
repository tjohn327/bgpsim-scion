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

//! SCION type definitions including ISD numbers, AS numbers, and link types.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::types::RouterId;

/// Isolation Domain identifier (16-bit).
///
/// ISDs are logical groupings of ASes with uniform trust environments.
/// ISD numbers must be globally unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IsdNumber(pub u16);

impl IsdNumber {
    /// The wildcard ISD (0) used to designate any ISD
    pub const WILDCARD: IsdNumber = IsdNumber(0);

    /// Check if this is the wildcard ISD
    pub fn is_wildcard(&self) -> bool {
        self.0 == 0
    }

    /// Check if this is a valid public ISD (64-4094)
    pub fn is_public(&self) -> bool {
        (64..=4094).contains(&self.0)
    }

    /// Check if this is a private ISD (16-63)
    pub fn is_private(&self) -> bool {
        (16..=63).contains(&self.0)
    }

    /// Check if this is reserved for documentation (1-15)
    pub fn is_documentation(&self) -> bool {
        (1..=15).contains(&self.0)
    }
}

impl From<u16> for IsdNumber {
    fn from(value: u16) -> Self {
        IsdNumber(value)
    }
}

impl fmt::Display for IsdNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// SCION AS number (48-bit identifier).
///
/// SCION AS numbers are different from BGP ASNs. They are 48-bit identifiers
/// that can be represented in decimal or colon-separated hex format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ScionAsn(u64);

impl ScionAsn {
    /// The wildcard AS (0) used to designate any core AS
    pub const WILDCARD: ScionAsn = ScionAsn(0);

    /// Maximum valid SCION AS number (48 bits)
    const MAX_ASN: u64 = 0xFFFF_FFFF_FFFF;

    /// Create a new SCION AS number. Returns None if the value exceeds 48 bits.
    pub fn new(value: u64) -> Option<Self> {
        if value <= Self::MAX_ASN {
            Some(ScionAsn(value))
        } else {
            None
        }
    }

    /// Create a SCION AS number without validation (for internal use)
    pub(crate) fn new_unchecked(value: u64) -> Self {
        ScionAsn(value & Self::MAX_ASN)
    }

    /// Get the raw u64 value
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Check if this is the wildcard AS
    pub fn is_wildcard(&self) -> bool {
        self.0 == 0
    }

    /// Check if this is a public AS (1-4294967295 or in 2:0:0 - 2:ffff:ffff range)
    pub fn is_public(&self) -> bool {
        (1..=0xFFFF_FFFF).contains(&self.0)
            || (0x0002_0000_0000..=0x0002_FFFF_FFFF).contains(&self.0)
    }

    /// Check if this is a private AS (in ffaa:0:0 - ffaa:ff:ffff range)
    pub fn is_private(&self) -> bool {
        (0xFFAA_0000_0000..=0xFFAA_00FF_FFFF).contains(&self.0)
    }

    /// Check if this is reserved for documentation (in ff00:0:0 - ff00:0:ffff range)
    pub fn is_documentation(&self) -> bool {
        (0xFF00_0000_0000..=0xFF00_0000_FFFF).contains(&self.0)
    }

    /// Parse from colon-separated hex format (e.g., "ff00:0:110")
    pub fn from_hex_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 3 {
            return None;
        }

        let high = u16::from_str_radix(parts[0], 16).ok()?;
        let mid = u16::from_str_radix(parts[1], 16).ok()?;
        let low = u32::from_str_radix(parts[2], 16).ok()?;

        let value = ((high as u64) << 32) | ((mid as u64) << 16) | (low as u64);
        Self::new(value)
    }

    /// Format as colon-separated hex (e.g., "ff00:0:110")
    pub fn to_hex_string(&self) -> String {
        let high = (self.0 >> 32) as u16;
        let mid = ((self.0 >> 16) & 0xFFFF) as u16;
        let low = (self.0 & 0xFFFF) as u32;
        format!("{:x}:{:x}:{:x}", high, mid, low)
    }
}

impl From<u32> for ScionAsn {
    fn from(value: u32) -> Self {
        ScionAsn(value as u64)
    }
}

impl From<u64> for ScionAsn {
    fn from(value: u64) -> Self {
        ScionAsn::new_unchecked(value)
    }
}

impl fmt::Display for ScionAsn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display in hex format if high bits are set, otherwise decimal
        if self.0 > 0xFFFF_FFFF {
            write!(f, "{}", self.to_hex_string())
        } else {
            write!(f, "{}", self.0)
        }
    }
}

/// Combined ISD-AS identifier.
///
/// This is the primary addressing unit in SCION, combining an ISD number
/// with an AS number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IsdAs {
    /// ISD number
    pub isd: IsdNumber,
    /// AS number
    pub asn: ScionAsn,
}

impl IsdAs {
    /// Create a new ISD-AS identifier
    pub fn new(isd: impl Into<IsdNumber>, asn: impl Into<ScionAsn>) -> Self {
        IsdAs {
            isd: isd.into(),
            asn: asn.into(),
        }
    }

    /// The wildcard ISD-AS (0-0) used for wildcard addressing
    pub const WILDCARD: IsdAs = IsdAs {
        isd: IsdNumber::WILDCARD,
        asn: ScionAsn::WILDCARD,
    };

    /// Create a wildcard address for a specific ISD (ISD-0)
    pub fn wildcard_as(isd: impl Into<IsdNumber>) -> Self {
        IsdAs {
            isd: isd.into(),
            asn: ScionAsn::WILDCARD,
        }
    }

    /// Check if this is a wildcard address (AS is 0)
    pub fn is_wildcard(&self) -> bool {
        self.asn.is_wildcard()
    }

    /// Check if both ISD and AS are wildcards
    pub fn is_full_wildcard(&self) -> bool {
        self.isd.is_wildcard() && self.asn.is_wildcard()
    }

    /// Parse from string format "ISD-AS" (e.g., "1-ff00:0:110" or "1-100")
    pub fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return None;
        }

        let isd = parts[0].parse::<u16>().ok()?.into();
        let asn = if parts[1].contains(':') {
            ScionAsn::from_hex_str(parts[1])?
        } else {
            ScionAsn::new(parts[1].parse::<u64>().ok()?)?
        };

        Some(IsdAs { isd, asn })
    }
}

impl fmt::Display for IsdAs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.isd, self.asn)
    }
}

/// SCION link types between ASes.
///
/// SCION distinguishes three types of links which determine routing behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScionLinkType {
    /// Core link connecting two core ASes (within or across ISDs)
    Core,
    /// Parent-child link creating a hierarchy within an ISD
    ParentChild,
    /// Peering link between any two ASes (can cross ISD boundaries)
    Peering,
}

impl fmt::Display for ScionLinkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScionLinkType::Core => write!(f, "Core"),
            ScionLinkType::ParentChild => write!(f, "ParentChild"),
            ScionLinkType::Peering => write!(f, "Peering"),
        }
    }
}

/// Interface ID (16-bit, unique within an AS).
///
/// Each link between SCION routers is identified by its corresponding
/// interface IDs on both sides. Interface IDs must be unique within
/// each AS but can be chosen independently without coordination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct InterfaceId(pub u16);

impl InterfaceId {
    /// The unspecified interface ID (0) used for special cases
    pub const UNSPECIFIED: InterfaceId = InterfaceId(0);

    /// Check if this is the unspecified interface ID
    pub fn is_unspecified(&self) -> bool {
        self.0 == 0
    }
}

impl From<u16> for InterfaceId {
    fn from(value: u16) -> Self {
        InterfaceId(value)
    }
}

impl fmt::Display for InterfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Information about a SCION interface to a neighbor.
///
/// This structure stores all the configuration needed for a link
/// to a neighboring AS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceInfo {
    /// Interface ID on this AS's side
    pub interface_id: InterfaceId,
    /// Neighbor's ISD-AS
    pub neighbor_isd_as: IsdAs,
    /// Type of link to the neighbor
    pub link_type: ScionLinkType,
    /// RouterId of the neighbor in the simulation
    pub neighbor_router: RouterId,
    /// Maximum Transmission Unit for this link
    pub mtu: u16,
}

impl InterfaceInfo {
    /// Create a new interface info
    pub fn new(
        interface_id: InterfaceId,
        neighbor_isd_as: IsdAs,
        link_type: ScionLinkType,
        neighbor_router: RouterId,
        mtu: u16,
    ) -> Self {
        InterfaceInfo {
            interface_id,
            neighbor_isd_as,
            link_type,
            neighbor_router,
            mtu,
        }
    }

    /// Check if this is a core link
    pub fn is_core_link(&self) -> bool {
        matches!(self.link_type, ScionLinkType::Core)
    }

    /// Check if this is a parent link (from child's perspective)
    pub fn is_parent_link(&self) -> bool {
        matches!(self.link_type, ScionLinkType::ParentChild)
    }

    /// Check if this is a peering link
    pub fn is_peering_link(&self) -> bool {
        matches!(self.link_type, ScionLinkType::Peering)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_isd_number() {
        let isd = IsdNumber(1);
        assert_eq!(isd.to_string(), "1");
        assert!(!isd.is_wildcard());

        let wildcard = IsdNumber::WILDCARD;
        assert!(wildcard.is_wildcard());

        let public = IsdNumber(100);
        assert!(public.is_public());

        let private = IsdNumber(20);
        assert!(private.is_private());

        let doc = IsdNumber(5);
        assert!(doc.is_documentation());
    }

    #[test]
    fn test_scion_asn() {
        let asn = ScionAsn::from(110u64);
        assert_eq!(asn.to_string(), "110");

        let asn_hex = ScionAsn::from(0xFF00_0000_0110u64);
        assert!(asn_hex.to_string().contains(':'));

        assert!(ScionAsn::WILDCARD.is_wildcard());
    }

    #[test]
    fn test_scion_asn_hex_parsing() {
        let asn = ScionAsn::from_hex_str("ff00:0:110").unwrap();
        assert_eq!(asn.to_hex_string(), "ff00:0:110");

        let asn2 = ScionAsn::from_hex_str("1:2:3").unwrap();
        assert_eq!(asn2.as_u64(), 0x0001_0002_0003);

        assert!(ScionAsn::from_hex_str("invalid").is_none());
        assert!(ScionAsn::from_hex_str("1:2:3:4").is_none());
    }

    #[test]
    fn test_isd_as() {
        let isd_as = IsdAs::new(1, 110u64);
        assert_eq!(isd_as.to_string(), "1-110");
        assert!(!isd_as.is_wildcard());

        let wildcard = IsdAs::wildcard_as(1);
        assert!(wildcard.is_wildcard());
        assert_eq!(wildcard.to_string(), "1-0");

        let full_wildcard = IsdAs::WILDCARD;
        assert!(full_wildcard.is_full_wildcard());
    }

    #[test]
    fn test_isd_as_parsing() {
        let isd_as = IsdAs::from_str("1-110").unwrap();
        assert_eq!(isd_as.isd.0, 1);
        assert_eq!(isd_as.asn.as_u64(), 110);

        let isd_as_hex = IsdAs::from_str("1-ff00:0:110").unwrap();
        assert_eq!(isd_as_hex.isd.0, 1);
        assert_eq!(isd_as_hex.asn.to_hex_string(), "ff00:0:110");

        assert!(IsdAs::from_str("invalid").is_none());
        assert!(IsdAs::from_str("1").is_none());
    }

    #[test]
    fn test_interface_id() {
        let if_id = InterfaceId(42);
        assert_eq!(if_id.to_string(), "42");
        assert!(!if_id.is_unspecified());

        let unspec = InterfaceId::UNSPECIFIED;
        assert!(unspec.is_unspecified());
    }

    #[test]
    fn test_scion_link_type() {
        let core = ScionLinkType::Core;
        assert_eq!(core.to_string(), "Core");

        let parent_child = ScionLinkType::ParentChild;
        assert_eq!(parent_child.to_string(), "ParentChild");

        let peering = ScionLinkType::Peering;
        assert_eq!(peering.to_string(), "Peering");
    }

    #[test]
    fn test_interface_info() {
        let neighbor_router = RouterId::from(10u32);
        let info = InterfaceInfo::new(
            InterfaceId(1),
            IsdAs::new(1, 110u64),
            ScionLinkType::Core,
            neighbor_router,
            1500,
        );

        assert!(info.is_core_link());
        assert!(!info.is_parent_link());
        assert!(!info.is_peering_link());
        assert_eq!(info.mtu, 1500);
    }

    #[test]
    fn test_serialization() {
        let isd_as = IsdAs::new(1, 110u64);
        let json = serde_json::to_string(&isd_as).unwrap();
        let deserialized: IsdAs = serde_json::from_str(&json).unwrap();
        assert_eq!(isd_as, deserialized);
    }
}
