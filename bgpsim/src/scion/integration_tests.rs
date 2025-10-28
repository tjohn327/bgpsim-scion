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

//! Integration tests for SCION Phases 1, 2, and 3.
//!
//! These tests validate that all components work together correctly:
//! - Phase 1: Core types, PCB structures, path segments
//! - Phase 2: BeaconStore, PathDatabase, ScionControlService
//! - Phase 3: Beaconing logic, PCB creation/extension, selection policies

#[cfg(test)]
mod tests {
    use crate::{
        scion::*,
        types::{RouterId, SimplePrefix},
    };

    /// Test end-to-end PCB creation, propagation, and storage workflow.
    ///
    /// This test validates:
    /// - Creating a SCION topology with multiple core ASes
    /// - PCB creation and extension across multiple hops
    /// - BeaconStore insertion and retrieval
    /// - PCB validation at each hop
    #[test]
    fn test_pcb_propagation_workflow() {
        // Setup: Create a simple topology with 3 core ASes in ISD 1
        // AS1 (110) <-> AS2 (120) <-> AS3 (130)

        let timestamp = 1000u32;

        // Core AS 1
        let as1 = IsdAs::new(1, 110u64);
        let mut cs1: ScionControlService<SimplePrefix> =
            ScionControlService::new(as1, true);

        // Core AS 2
        let as2 = IsdAs::new(1, 120u64);
        let mut cs2: ScionControlService<SimplePrefix> =
            ScionControlService::new(as2, true);

        // Core AS 3
        let as3 = IsdAs::new(1, 130u64);
        let mut cs3: ScionControlService<SimplePrefix> =
            ScionControlService::new(as3, true);

        // Setup interfaces
        // AS1 -> AS2
        cs1.add_interface(InterfaceInfo::new(
            InterfaceId(1),
            as2,
            ScionLinkType::Core,
            RouterId::from(2u32),
            1500,
        ))
        .unwrap();

        // AS2 -> AS1 and AS2 -> AS3
        cs2.add_interface(InterfaceInfo::new(
            InterfaceId(1),
            as1,
            ScionLinkType::Core,
            RouterId::from(1u32),
            1500,
        ))
        .unwrap();
        cs2.add_interface(InterfaceInfo::new(
            InterfaceId(2),
            as3,
            ScionLinkType::Core,
            RouterId::from(3u32),
            1500,
        ))
        .unwrap();

        // AS3 -> AS2
        cs3.add_interface(InterfaceInfo::new(
            InterfaceId(1),
            as2,
            ScionLinkType::Core,
            RouterId::from(2u32),
            1500,
        ))
        .unwrap();

        // Step 1: AS1 creates initial PCB
        let pcb1 = create_initial_pcb(as1, timestamp, InterfaceId(1), 1500);

        // Validate PCB at AS1
        assert!(validate_pcb(&pcb1, timestamp, true).is_ok());
        assert_eq!(pcb1.path_length(), 1);
        assert_eq!(pcb1.get_origin(), Some(as1));

        // Step 2: AS2 receives and extends the PCB
        let pcb2 = extend_pcb(
            pcb1.clone(),
            as2,
            InterfaceId(1), // Ingress from AS1
            InterfaceId(2), // Egress to AS3
            1500,
            timestamp,
        );

        // Validate extended PCB
        assert!(validate_pcb(&pcb2, timestamp, true).is_ok());
        assert_eq!(pcb2.path_length(), 2);
        assert_eq!(pcb2.get_origin(), Some(as1));
        assert_eq!(pcb2.get_last_as(), Some(as2));

        // Store in AS2's beacon store
        assert!(cs2.beacon_store.insert(pcb1.clone()));
        assert_eq!(cs2.beacon_store.total_count(), 1);

        // Step 3: AS3 receives and extends the PCB
        let pcb3 = extend_pcb(
            pcb2.clone(),
            as3,
            InterfaceId(1), // Ingress from AS2
            InterfaceId::UNSPECIFIED, // No egress (end of path)
            1500,
            timestamp,
        );

        // Validate final PCB
        assert!(validate_pcb(&pcb3, timestamp, true).is_ok());
        assert_eq!(pcb3.path_length(), 3);
        assert_eq!(pcb3.get_origin(), Some(as1));
        assert_eq!(pcb3.get_last_as(), Some(as3));

        // Store in AS3's beacon store
        assert!(cs3.beacon_store.insert(pcb2.clone()));
        assert_eq!(cs3.beacon_store.total_count(), 1);

        // Verify AS path
        let as_path = pcb3.get_as_path();
        assert_eq!(as_path.len(), 3);
        assert_eq!(as_path[0], as1);
        assert_eq!(as_path[1], as2);
        assert_eq!(as_path[2], as3);

        // Verify no loops
        assert!(!pcb3.has_loop());
    }

    /// Test PCB selection policy with multiple beacons.
    ///
    /// This test validates:
    /// - Multiple PCBs stored in BeaconStore
    /// - Selection policy correctly choosing best PCBs
    /// - Retrieval by source AS
    #[test]
    fn test_beacon_selection_workflow() {
        let timestamp = 1000u32;
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(IsdAs::new(1, 100u64), true);

        // Create multiple PCBs from different sources with different path lengths
        // Short path: AS1 -> AS2
        let as1 = IsdAs::new(1, 110u64);
        let as2 = IsdAs::new(1, 100u64);
        let pcb_short = extend_pcb(
            create_initial_pcb(as1, timestamp, InterfaceId(1), 1500),
            as2,
            InterfaceId(1),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );

        // Medium path: AS3 -> AS4 -> AS2
        let as3 = IsdAs::new(1, 130u64);
        let as4 = IsdAs::new(1, 140u64);
        let pcb_medium = extend_pcb(
            extend_pcb(
                create_initial_pcb(as3, timestamp, InterfaceId(1), 1500),
                as4,
                InterfaceId(1),
                InterfaceId(2),
                1500,
                timestamp,
            ),
            as2,
            InterfaceId(2),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );

        // Long path: AS5 -> AS6 -> AS7 -> AS2
        let as5 = IsdAs::new(1, 150u64);
        let as6 = IsdAs::new(1, 160u64);
        let as7 = IsdAs::new(1, 170u64);
        let pcb_long = extend_pcb(
            extend_pcb(
                extend_pcb(
                    create_initial_pcb(as5, timestamp, InterfaceId(1), 1500),
                    as6,
                    InterfaceId(1),
                    InterfaceId(2),
                    1500,
                    timestamp,
                ),
                as7,
                InterfaceId(2),
                InterfaceId(3),
                1500,
                timestamp,
            ),
            as2,
            InterfaceId(3),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );

        // Store all PCBs
        assert!(cs.beacon_store.insert(pcb_short.clone()));
        assert!(cs.beacon_store.insert(pcb_medium.clone()));
        assert!(cs.beacon_store.insert(pcb_long.clone()));
        assert_eq!(cs.beacon_store.total_count(), 3);
        assert_eq!(cs.beacon_store.source_count(), 3);

        // Get all beacons
        let all_beacons = cs.beacon_store.get_all();
        assert_eq!(all_beacons.len(), 3);

        // Test selection policy - select 2 best (shortest)
        let policy = SimpleSelectionPolicy;
        let selected = policy.select_pcbs(&all_beacons, 2);
        assert_eq!(selected.len(), 2);

        // Verify selected PCBs are the shortest ones
        let selected_lengths: Vec<usize> =
            selected.iter().map(|&i| all_beacons[i].path_length()).collect();
        assert!(selected_lengths.contains(&2)); // pcb_short
        assert!(selected_lengths.contains(&3)); // pcb_medium
        assert!(!selected_lengths.contains(&4)); // pcb_long not selected

        // Test retrieval by source
        let from_as1 = cs.beacon_store.get_by_source(&as1);
        assert_eq!(from_as1.len(), 1);
        assert_eq!(from_as1[0].path_length(), 2);

        let from_as5 = cs.beacon_store.get_by_source(&as5);
        assert_eq!(from_as5.len(), 1);
        assert_eq!(from_as5[0].path_length(), 4);
    }

    /// Test path segment conversion from PCBs.
    ///
    /// This test validates:
    /// - Converting PCBs to path segments
    /// - Different segment types (up, down, core)
    /// - Segment registration in PathDatabase
    /// - Segment lookup by destination
    #[test]
    fn test_path_segment_workflow() {
        let timestamp = 1000u32;

        // Create a PCB: AS1 (core) -> AS2 (intermediary) -> AS3 (non-core)
        // This represents beaconing from core outward to non-core
        let as1 = IsdAs::new(1, 110u64); // Core AS
        let as2 = IsdAs::new(1, 120u64); // Intermediary AS
        let as3 = IsdAs::new(1, 130u64); // Non-core AS

        let pcb = extend_pcb(
            extend_pcb(
                create_initial_pcb(as1, timestamp, InterfaceId(1), 1500),
                as2,
                InterfaceId(1),
                InterfaceId(2),
                1500,
                timestamp,
            ),
            as3,
            InterfaceId(2),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );

        assert_eq!(pcb.path_length(), 3);

        // Create path segments from PCB
        // PCB as_path is [110, 120, 130] (beaconing from core to non-core)
        let up_segment: PathSegment<SimplePrefix> = PathSegment::from_pcb(&pcb, SegmentType::Up);
        let down_segment: PathSegment<SimplePrefix> = up_segment.reverse(); // Proper way to create down-segment
        let core_segment: PathSegment<SimplePrefix> =
            PathSegment::from_pcb(&pcb, SegmentType::Core);

        // Verify segment properties
        // Up-segment: represents path FROM non-core (130) TO core (110) for forwarding
        assert_eq!(up_segment.segment_type, SegmentType::Up);
        assert_eq!(up_segment.source(), Some(as3)); // Up: from non-core AS3
        assert_eq!(up_segment.destination(), Some(as1)); // Up: to core AS1

        // Down-segment: reversed up-segment, FROM core (110) TO non-core (130) for forwarding
        assert_eq!(down_segment.segment_type, SegmentType::Down);
        assert_eq!(down_segment.source(), Some(as1)); // Down: from core AS1
        assert_eq!(down_segment.destination(), Some(as3)); // Down: to non-core AS3

        // Core-segment: can use same PCB, goes from first to last in beaconing direction
        assert_eq!(core_segment.segment_type, SegmentType::Core);
        assert_eq!(core_segment.source(), Some(as1));
        assert_eq!(core_segment.destination(), Some(as3));

        // Register segments in path database
        let mut path_db: PathDatabase<SimplePrefix> = PathDatabase::default();

        assert!(path_db.register_segment(up_segment.clone()));
        assert!(path_db.register_segment(down_segment.clone()));
        assert!(path_db.register_segment(core_segment.clone()));

        assert_eq!(path_db.total_count(), 3);

        // Test lookups
        // Up-segment: from as3 (non-core) to as1 (core)
        // lookup_up_segments looks for segments by destination
        let up_to_as1 = path_db.lookup_up_segments(&as1);
        assert_eq!(up_to_as1.len(), 1);
        assert_eq!(up_to_as1[0].segment_type, SegmentType::Up);

        // Down-segment: from as1 (core) to as3 (non-core)
        // lookup_down_segments looks for segments by source
        let down_from_as1 = path_db.lookup_down_segments(&as1);
        assert_eq!(down_from_as1.len(), 1);
        assert_eq!(down_from_as1[0].segment_type, SegmentType::Down);

        // Core-segment: from as1 to as3
        let core_to_as3 = path_db.lookup_core_segments(Some(&as1), Some(&as3));
        assert_eq!(core_to_as3.len(), 1);
        assert_eq!(core_to_as3[0].segment_type, SegmentType::Core);
    }

    /// Test complete control service workflow with interface management.
    ///
    /// This test validates:
    /// - ScionControlService creation and configuration
    /// - Interface management (add, remove, lookup)
    /// - Integration with BeaconStore and PathDatabase
    /// - Expiration handling
    #[test]
    fn test_control_service_complete_workflow() {
        let isd_as = IsdAs::new(1, 110u64);
        let mut cs: ScionControlService<SimplePrefix> =
            ScionControlService::new(isd_as, true);

        assert_eq!(cs.isd_as, isd_as);
        assert!(cs.is_core);
        assert_eq!(cs.interface_count(), 0);

        // Add multiple interfaces
        let neighbor1 = IsdAs::new(1, 120u64);
        let neighbor2 = IsdAs::new(1, 130u64);
        let neighbor3 = IsdAs::new(1, 140u64);

        cs.add_interface(InterfaceInfo::new(
            InterfaceId(1),
            neighbor1,
            ScionLinkType::Core,
            RouterId::from(10u32),
            1500,
        ))
        .unwrap();

        cs.add_interface(InterfaceInfo::new(
            InterfaceId(2),
            neighbor2,
            ScionLinkType::Core,
            RouterId::from(20u32),
            1500,
        ))
        .unwrap();

        cs.add_interface(InterfaceInfo::new(
            InterfaceId(3),
            neighbor3,
            ScionLinkType::ParentChild,
            RouterId::from(30u32),
            1400,
        ))
        .unwrap();

        assert_eq!(cs.interface_count(), 3);

        // Test interface lookups
        let core_ifs = cs.get_core_interfaces();
        assert_eq!(core_ifs.len(), 2);

        let parent_ifs = cs.get_parent_interfaces();
        assert_eq!(parent_ifs.len(), 1);
        assert_eq!(parent_ifs[0].mtu, 1400);

        // Test interface lookup by ID
        let if1 = cs.get_interface_by_id(InterfaceId(1)).unwrap();
        assert_eq!(if1.neighbor_isd_as, neighbor1);

        // Test neighbor lookup by interface ID
        let neighbor_router = cs.get_neighbor_by_interface_id(InterfaceId(2));
        assert_eq!(neighbor_router, Some(RouterId::from(20u32)));

        // Add PCBs to beacon store
        let pcb1 = create_initial_pcb(neighbor1, 1000, InterfaceId(1), 1500);
        let pcb2 = create_initial_pcb(neighbor2, 1000, InterfaceId(2), 1500);

        assert!(cs.beacon_store.insert(pcb1));
        assert!(cs.beacon_store.insert(pcb2));
        assert_eq!(cs.beacon_store.total_count(), 2);

        // Add segments to path database
        let segment = PathSegment::from_pcb(
            &create_initial_pcb(neighbor1, 1000, InterfaceId(1), 1500),
            SegmentType::Core,
        );
        assert!(cs.path_database.register_segment(segment));
        assert_eq!(cs.path_database.total_count(), 1);

        // Test expiration (nothing should expire at current time)
        let (expired_pcbs, expired_segs) = cs.clear_expired(1100);
        assert_eq!(expired_pcbs, 0);
        assert_eq!(expired_segs, 0);

        // Remove an interface
        let removed = cs.remove_interface(RouterId::from(10u32));
        assert!(removed.is_some());
        assert_eq!(cs.interface_count(), 2);
        assert!(!cs.has_interface_id(InterfaceId(1)));
    }

    /// Test ForwardingPath creation from path segments.
    ///
    /// This test validates:
    /// - Creating forwarding paths from segments
    /// - Path validation (valley-free, loops)
    /// - Segment connectivity checking
    /// - MTU calculation
    #[test]
    fn test_forwarding_path_construction() {
        let timestamp = 1000u32;

        // Create topology: AS1 (non-core) -> AS2 (core) -> AS3 (non-core)
        let as1 = IsdAs::new(1, 110u64);
        let as2_core = IsdAs::new(1, 120u64);
        let as3 = IsdAs::new(1, 130u64);

        // Create up-segment from AS1 to AS2 (core)
        // PCB beaconing goes from core to non-core: [120, 110]
        let pcb_up = extend_pcb(
            create_initial_pcb(as2_core, timestamp, InterfaceId(1), 1500),
            as1,
            InterfaceId(1),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );
        let up_seg: PathSegment<SimplePrefix> =
            PathSegment::from_pcb(&pcb_up, SegmentType::Up);
        // up_seg: as_path=[120,110], source=110 (non-core), dest=120 (core)

        // Create down-segment from AS2 (core) to AS3
        // First create up-segment from AS3 to AS2, then reverse it
        let pcb_for_down = extend_pcb(
            create_initial_pcb(as2_core, timestamp, InterfaceId(2), 1400),
            as3,
            InterfaceId(2),
            InterfaceId::UNSPECIFIED,
            1400,
            timestamp,
        );
        let temp_up = PathSegment::from_pcb(&pcb_for_down, SegmentType::Up);
        let down_seg = temp_up.reverse();
        // down_seg: source=120 (core), dest=130 (non-core)

        // Verify segments can connect
        assert!(up_seg.can_connect(&down_seg));

        // Create forwarding path
        let path_result = ForwardingPath::new(Some(up_seg.clone()), None, Some(down_seg.clone()));
        assert!(path_result.is_ok());

        let path = path_result.unwrap();

        // Verify path properties
        assert!(path.is_valley_free());
        assert!(!path.has_loop());
        assert!(path.validate().is_ok());

        // Verify AS path
        assert_eq!(path.as_path.len(), 3);
        assert_eq!(path.as_path[0], as1);
        assert_eq!(path.as_path[1], as2_core);
        assert_eq!(path.as_path[2], as3);

        // Verify MTU (should be minimum of all segments)
        assert_eq!(path.mtu, 1400); // Limited by down_seg

        // Verify segments are present
        assert!(path.up_segment.is_some());
        assert!(path.down_segment.is_some());
        assert!(path.core_segment.is_none());
        assert!(path.peering_shortcut.is_none());
    }

    /// Test the complete end-to-end workflow across all three phases.
    ///
    /// This mega-test validates:
    /// - Full topology setup with multiple ASes
    /// - PCB creation and multi-hop propagation
    /// - Beacon storage at intermediate ASes
    /// - Path segment creation and registration
    /// - Forwarding path construction
    /// - Selection policies
    /// - Validation at each step
    #[test]
    fn test_complete_end_to_end_workflow() {
        let timestamp = 1000u32;

        // Topology:
        // Non-core AS1 (110) -- Core AS2 (120) -- Core AS3 (130) -- Non-core AS4 (140)
        //                            |
        //                       Core AS5 (150)

        let as1 = IsdAs::new(1, 110u64); // Non-core
        let as2 = IsdAs::new(1, 120u64); // Core
        let as3 = IsdAs::new(1, 130u64); // Core
        let as4 = IsdAs::new(1, 140u64); // Non-core
        let as5 = IsdAs::new(1, 150u64); // Core

        // Create control services
        let _cs1: ScionControlService<SimplePrefix> =
            ScionControlService::new(as1, false);
        let mut cs2: ScionControlService<SimplePrefix> =
            ScionControlService::new(as2, true);
        let mut cs3: ScionControlService<SimplePrefix> =
            ScionControlService::new(as3, true);
        let _cs4: ScionControlService<SimplePrefix> =
            ScionControlService::new(as4, false);
        let mut cs5: ScionControlService<SimplePrefix> =
            ScionControlService::new(as5, true);

        // Setup interfaces (simplified - just recording connectivity)
        // AS2 has connections to AS1, AS3, AS5
        cs2.add_interface(InterfaceInfo::new(
            InterfaceId(1),
            as1,
            ScionLinkType::ParentChild,
            RouterId::from(1u32),
            1500,
        ))
        .unwrap();
        cs2.add_interface(InterfaceInfo::new(
            InterfaceId(2),
            as3,
            ScionLinkType::Core,
            RouterId::from(3u32),
            1500,
        ))
        .unwrap();
        cs2.add_interface(InterfaceInfo::new(
            InterfaceId(3),
            as5,
            ScionLinkType::Core,
            RouterId::from(5u32),
            1500,
        ))
        .unwrap();

        // === Phase 1: Core Beaconing ===

        // AS2 creates initial PCB and propagates to AS3
        let pcb_2to3 = create_initial_pcb(as2, timestamp, InterfaceId(2), 1500);
        assert!(validate_pcb(&pcb_2to3, timestamp, true).is_ok());

        // AS3 receives and extends
        let pcb_at_3 = extend_pcb(
            pcb_2to3.clone(),
            as3,
            InterfaceId(1),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );
        assert!(validate_pcb(&pcb_at_3, timestamp, true).is_ok());
        assert_eq!(pcb_at_3.path_length(), 2);

        // AS3 stores the PCB
        assert!(cs3.beacon_store.insert(pcb_2to3.clone()));

        // AS2 also creates PCB for AS5
        let pcb_2to5 = create_initial_pcb(as2, timestamp, InterfaceId(3), 1500);
        let pcb_at_5 = extend_pcb(
            pcb_2to5.clone(),
            as5,
            InterfaceId(1),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );
        assert!(cs5.beacon_store.insert(pcb_2to5));

        // === Phase 2: Selection and Storage ===

        // AS5 creates its own PCB to AS2
        let pcb_5to2 = create_initial_pcb(as5, timestamp, InterfaceId(1), 1500);
        let _pcb_5to2_extended = extend_pcb(
            pcb_5to2.clone(),
            as2,
            InterfaceId(3),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );

        // AS2 receives PCBs from multiple sources
        assert!(cs2.beacon_store.insert(pcb_5to2.clone()));
        assert!(cs2.beacon_store.insert(pcb_at_5.clone()));

        // Test selection policy
        let all_pcbs = cs2.beacon_store.get_all();
        let policy = SimpleSelectionPolicy;
        let selected = policy.select_pcbs(&all_pcbs, 1);
        assert_eq!(selected.len(), 1);

        // === Phase 3: Path Segment Registration ===

        // Convert PCBs to path segments
        let core_seg_2to3: PathSegment<SimplePrefix> =
            PathSegment::from_pcb(&pcb_at_3, SegmentType::Core);
        let core_seg_2to5: PathSegment<SimplePrefix> =
            PathSegment::from_pcb(&pcb_at_5, SegmentType::Core);

        // Register in path database
        assert!(cs3.path_database.register_segment(core_seg_2to3.clone()));
        assert!(cs2.path_database.register_segment(core_seg_2to5.clone()));

        // Verify lookups work
        let segs_to_3 = cs3.path_database.lookup_core_segments(Some(&as2), Some(&as3));
        assert_eq!(segs_to_3.len(), 1);

        // === Phase 4: Forwarding Path Construction ===

        // Create up-segment from AS1 to AS2
        // PCB beaconing goes from core (AS2) to non-core (AS1)
        let pcb_2to1 = extend_pcb(
            create_initial_pcb(as2, timestamp, InterfaceId(1), 1500),
            as1,
            InterfaceId(1),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );
        // Up-segment: as_path=[120,110], source=110 (non-core), dest=120 (core)
        let up_seg_1to2: PathSegment<SimplePrefix> =
            PathSegment::from_pcb(&pcb_2to1, SegmentType::Up);

        // Create down-segment from AS3 to AS4
        // First create up-segment from AS4 to AS3, then reverse it
        // PCB beaconing goes from core (AS3) to non-core (AS4)
        let pcb_3to4 = extend_pcb(
            create_initial_pcb(as3, timestamp, InterfaceId(2), 1500),
            as4,
            InterfaceId(2),
            InterfaceId::UNSPECIFIED,
            1500,
            timestamp,
        );
        let temp_up = PathSegment::from_pcb(&pcb_3to4, SegmentType::Up);
        // Down-segment created by reversing up-segment
        // down_seg: source=130 (core), dest=140 (non-core)
        let down_seg_3to4 = temp_up.reverse();

        // Construct complete path AS1 -> AS2 -> AS3 -> AS4
        let complete_path = ForwardingPath::new(
            Some(up_seg_1to2),
            Some(core_seg_2to3),
            Some(down_seg_3to4),
        );

        assert!(complete_path.is_ok());
        let path = complete_path.unwrap();

        // Final validations
        assert!(path.validate().is_ok());
        assert!(path.is_valley_free());
        assert!(!path.has_loop());
        assert_eq!(path.as_path.len(), 4);
        assert_eq!(path.as_path[0], as1);
        assert_eq!(path.as_path[1], as2);
        assert_eq!(path.as_path[2], as3);
        assert_eq!(path.as_path[3], as4);

        // Verify beacon counts across all ASes
        assert_eq!(cs2.beacon_store.total_count(), 2);
        assert_eq!(cs3.beacon_store.total_count(), 1);
        assert_eq!(cs5.beacon_store.total_count(), 1);

        // Verify path database counts
        assert_eq!(cs2.path_database.total_count(), 1);
        assert_eq!(cs3.path_database.total_count(), 1);
    }
}

