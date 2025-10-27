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

//! SCION Control Plane events.
//!
//! These events represent SCION-specific control plane operations such as
//! beacon propagation, path registration, and path lookup.

use serde::{Deserialize, Serialize};

use crate::scion::path_segment::{PathSegment, SegmentType};
use crate::scion::pcb::Pcb;
use crate::scion::types::IsdAs;
use crate::types::{Prefix, RouterId};

/// SCION Control Plane events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "P: Serialize",
    deserialize = "P: for<'a> serde::Deserialize<'a>"
))]
pub enum ScionEvent<P: Prefix> {
    /// PCB propagation from one AS to another
    BeaconPropagation {
        /// Source AS (sender)
        src: RouterId,
        /// Destination AS (receiver)
        dst: RouterId,
        /// The PCB being propagated
        pcb: Pcb<P>,
    },

    /// Path segment registration
    SegmentRegistration {
        /// AS registering the segment
        registering_as: RouterId,
        /// Target core AS where segment is being registered
        target_core: RouterId,
        /// The path segment to register
        segment: PathSegment<P>,
    },

    /// Path lookup request
    PathLookupRequest {
        /// Source AS making the request
        src: RouterId,
        /// Destination ISD-AS to reach
        dst_isd_as: IsdAs,
        /// Type of segment being requested
        segment_type: SegmentType,
    },

    /// Path lookup response
    PathLookupResponse {
        /// Requesting AS
        requester: RouterId,
        /// AS that responded
        responder: RouterId,
        /// Segments returned
        segments: Vec<PathSegment<P>>,
    },

    /// Periodic trigger for core beaconing
    CoreBeaconTrigger {
        /// Core AS that should initiate beaconing
        core_as: RouterId,
    },

    /// Periodic trigger for intra-ISD beaconing
    IntraIsdBeaconTrigger {
        /// AS that should propagate beacons
        as_router: RouterId,
    },

    /// Periodic trigger for path segment registration
    RegistrationTrigger {
        /// AS that should register segments
        as_router: RouterId,
    },
}

impl<P: Prefix> ScionEvent<P> {
    /// Get the primary router involved in this event
    pub fn primary_router(&self) -> RouterId {
        match self {
            ScionEvent::BeaconPropagation { dst, .. } => *dst,
            ScionEvent::SegmentRegistration { target_core, .. } => *target_core,
            ScionEvent::PathLookupRequest { src, .. } => *src,
            ScionEvent::PathLookupResponse { requester, .. } => *requester,
            ScionEvent::CoreBeaconTrigger { core_as } => *core_as,
            ScionEvent::IntraIsdBeaconTrigger { as_router } => *as_router,
            ScionEvent::RegistrationTrigger { as_router } => *as_router,
        }
    }

    /// Get source and destination routers if applicable
    pub fn routers(&self) -> Option<(RouterId, RouterId)> {
        match self {
            ScionEvent::BeaconPropagation { src, dst, .. } => Some((*src, *dst)),
            ScionEvent::SegmentRegistration {
                registering_as,
                target_core,
                ..
            } => Some((*registering_as, *target_core)),
            ScionEvent::PathLookupResponse {
                requester,
                responder,
                ..
            } => Some((*responder, *requester)),
            _ => None,
        }
    }

    /// Check if this is a trigger event (periodic operation)
    pub fn is_trigger(&self) -> bool {
        matches!(
            self,
            ScionEvent::CoreBeaconTrigger { .. }
                | ScionEvent::IntraIsdBeaconTrigger { .. }
                | ScionEvent::RegistrationTrigger { .. }
        )
    }

    /// Check if this is a beacon-related event
    pub fn is_beacon_event(&self) -> bool {
        matches!(
            self,
            ScionEvent::BeaconPropagation { .. }
                | ScionEvent::CoreBeaconTrigger { .. }
                | ScionEvent::IntraIsdBeaconTrigger { .. }
        )
    }

    /// Check if this is a registration-related event
    pub fn is_registration_event(&self) -> bool {
        matches!(
            self,
            ScionEvent::SegmentRegistration { .. } | ScionEvent::RegistrationTrigger { .. }
        )
    }

    /// Check if this is a lookup-related event
    pub fn is_lookup_event(&self) -> bool {
        matches!(
            self,
            ScionEvent::PathLookupRequest { .. } | ScionEvent::PathLookupResponse { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scion::pcb::SegmentInfo;
    use crate::types::SimplePrefix;

    #[test]
    fn test_beacon_propagation_event() {
        let info = SegmentInfo::new(1000, 12345);
        let pcb = Pcb::<SimplePrefix>::new(info);

        let event = ScionEvent::BeaconPropagation {
            src: RouterId::from(1u32),
            dst: RouterId::from(2u32),
            pcb,
        };

        assert!(event.is_beacon_event());
        assert!(!event.is_trigger());
        assert_eq!(event.primary_router(), RouterId::from(2u32));

        let routers = event.routers().unwrap();
        assert_eq!(routers.0, RouterId::from(1u32));
        assert_eq!(routers.1, RouterId::from(2u32));
    }

    #[test]
    fn test_trigger_events() {
        let core_trigger = ScionEvent::<SimplePrefix>::CoreBeaconTrigger {
            core_as: RouterId::from(1u32),
        };

        assert!(core_trigger.is_trigger());
        assert!(core_trigger.is_beacon_event());
        assert_eq!(core_trigger.primary_router(), RouterId::from(1u32));

        let intra_trigger = ScionEvent::<SimplePrefix>::IntraIsdBeaconTrigger {
            as_router: RouterId::from(2u32),
        };

        assert!(intra_trigger.is_trigger());
        assert!(intra_trigger.is_beacon_event());

        let reg_trigger = ScionEvent::<SimplePrefix>::RegistrationTrigger {
            as_router: RouterId::from(3u32),
        };

        assert!(reg_trigger.is_trigger());
        assert!(reg_trigger.is_registration_event());
    }

    #[test]
    fn test_lookup_events() {
        let request = ScionEvent::<SimplePrefix>::PathLookupRequest {
            src: RouterId::from(1u32),
            dst_isd_as: IsdAs::new(1, 110u64),
            segment_type: SegmentType::Up,
        };

        assert!(request.is_lookup_event());
        assert!(!request.is_trigger());

        let response = ScionEvent::<SimplePrefix>::PathLookupResponse {
            requester: RouterId::from(1u32),
            responder: RouterId::from(2u32),
            segments: Vec::new(),
        };

        assert!(response.is_lookup_event());
    }

    #[test]
    fn test_serialization() {
        let event = ScionEvent::<SimplePrefix>::CoreBeaconTrigger {
            core_as: RouterId::from(1u32),
        };

        let json = serde_json::to_string(&event).unwrap();
        let deserialized: ScionEvent<SimplePrefix> = serde_json::from_str(&json).unwrap();

        assert_eq!(event, deserialized);
    }
}
