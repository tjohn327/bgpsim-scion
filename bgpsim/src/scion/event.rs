// SCION control plane events (AS-level communication)
//
// Events are exchanged between ScionControlServices, not individual routers.
// This ensures events scale with O(ASes) rather than O(Routers).

use super::pcb::Pcb;
use super::path_db::{PathSegment, SegmentType};
use super::types::ScionLinkType;
use std::sync::Arc;

/// SCION control plane events (AS-level)
///
/// These events are exchanged between SCION control services (one per ISD-AS).
/// All PCBs and segments are shared via Arc for zero-copy efficiency.
///
/// Note: Does not implement PartialEq/Eq/Hash due to Arc<Pcb>/Arc<PathSegment> fields.
/// Event queues use priority-based ordering, not equality checks.
#[derive(Debug, Clone)]
pub enum ScionEvent {
    /// Batched PCB propagation to neighboring AS
    ///
    /// Multiple PCBs are batched into a single event for efficiency.
    /// This reduces event count by ~20x compared to individual PCB events.
    BeaconBatch {
        /// PCBs being propagated (shared via Arc for zero-copy)
        pcbs: Vec<Arc<Pcb>>,

        /// Type of link over which PCBs are sent
        link_type: ScionLinkType,
    },

    /// Path segment registration at core AS
    ///
    /// Non-core ASes register down segments with core ASes so that
    /// other ASes can discover paths to them.
    SegmentRegistration {
        /// Segments being registered (shared via Arc)
        segments: Vec<Arc<PathSegment>>,

        /// Type of segments (typically Down)
        segment_type: SegmentType,
    },

    /// Periodic beaconing timeout (triggers new propagation)
    ///
    /// This is a self-scheduled event that triggers the control service
    /// to select, extend, and propagate PCBs to neighbors.
    BeaconTimeout {
        /// Which beaconing process to run
        interval_type: BeaconIntervalType,
    },

    /// Registration timeout (triggers segment registration)
    ///
    /// Periodically, non-core ASes select PCBs from their beacon store,
    /// terminate them into path segments, and register them with core ASes.
    RegistrationTimeout,
}

/// Beaconing interval types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeaconIntervalType {
    /// Intra-ISD beaconing (core → non-core, typically 5 seconds)
    ///
    /// Core ASes initiate PCBs and send to children. Non-core ASes
    /// extend and forward to their children.
    IntraIsd,

    /// Core beaconing (core ↔ core, typically 60 seconds)
    ///
    /// Core ASes exchange PCBs to discover paths between cores
    /// within the same ISD or across ISDs.
    Core,
}

impl ScionEvent {
    /// Create a beacon batch event
    pub fn beacon_batch(pcbs: Vec<Arc<Pcb>>, link_type: ScionLinkType) -> Self {
        Self::BeaconBatch { pcbs, link_type }
    }

    /// Create a segment registration event
    pub fn segment_registration(
        segments: Vec<Arc<PathSegment>>,
        segment_type: SegmentType,
    ) -> Self {
        Self::SegmentRegistration {
            segments,
            segment_type,
        }
    }

    /// Create an intra-ISD beacon timeout event
    pub fn intra_isd_timeout() -> Self {
        Self::BeaconTimeout {
            interval_type: BeaconIntervalType::IntraIsd,
        }
    }

    /// Create a core beacon timeout event
    pub fn core_timeout() -> Self {
        Self::BeaconTimeout {
            interval_type: BeaconIntervalType::Core,
        }
    }

    /// Create a registration timeout event
    pub fn registration_timeout() -> Self {
        Self::RegistrationTimeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scion::pcb::{AsEntry, HopEntry, SegmentInfo};
    use crate::scion::types::{IsdAs, InterfaceId};

    fn create_test_pcb() -> Arc<Pcb> {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000, 42));
        pcb.extend(AsEntry::new(
            IsdAs::new(1, 100),
            Some(IsdAs::new(1, 101)),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        Arc::new(pcb)
    }

    #[test]
    fn test_beacon_batch_creation() {
        let pcb1 = create_test_pcb();
        let pcb2 = create_test_pcb();
        let event = ScionEvent::beacon_batch(vec![pcb1, pcb2], ScionLinkType::Parent);

        match event {
            ScionEvent::BeaconBatch { pcbs, link_type } => {
                assert_eq!(pcbs.len(), 2);
                assert_eq!(link_type, ScionLinkType::Parent);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_timeout_events() {
        let intra_event = ScionEvent::intra_isd_timeout();
        match intra_event {
            ScionEvent::BeaconTimeout { interval_type } => {
                assert_eq!(interval_type, BeaconIntervalType::IntraIsd);
            }
            _ => panic!("Wrong event type"),
        }

        let core_event = ScionEvent::core_timeout();
        match core_event {
            ScionEvent::BeaconTimeout { interval_type } => {
                assert_eq!(interval_type, BeaconIntervalType::Core);
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_registration_timeout() {
        let event = ScionEvent::registration_timeout();
        assert!(matches!(event, ScionEvent::RegistrationTimeout));
    }

    #[test]
    fn test_segment_registration() {
        let segment = Arc::new(PathSegment::new(
            SegmentType::Down,
            Pcb::with_segment_info(SegmentInfo::with_values(1000, 42)),
        ));

        let event = ScionEvent::segment_registration(vec![segment], SegmentType::Down);

        match event {
            ScionEvent::SegmentRegistration {
                segments,
                segment_type,
            } => {
                assert_eq!(segments.len(), 1);
                assert_eq!(segment_type, SegmentType::Down);
            }
            _ => panic!("Wrong event type"),
        }
    }
}
