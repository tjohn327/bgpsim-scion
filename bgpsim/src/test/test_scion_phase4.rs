// Phase 4 Tests: Path Segment Construction and Registration
//
// These tests verify the SCION segment registration implementation:
// - PCB termination (final AS entry with None values)
// - Up segment registration (non-core, local storage)
// - Down segment registration (non-core, sent to core)
// - Core segment registration (core, local storage)
// - Down segment validation at core AS
// - Full registration flow with RegistrationTimeout events

use crate::event::Event;
use crate::network::Network;
use crate::scion::*;
use crate::types::SimplePrefix;
use std::sync::Arc;

// ===== PCB Termination Tests (§4.1.1) =====

#[test]
fn test_pcb_termination_final_entry() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Core AS
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Create PCB and store in child's beacon store
    let mut pcb = Pcb::new(core_as);
    pcb.extend(AsEntry::new(
        core_as,
        Some(child_as),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));

    let cs = &mut net.scion_services.get_mut(&child_as).unwrap();
    cs.beacon_store.insert(Arc::new(pcb));

    // Register up segments (which terminates PCBs)
    let segments = cs.register_up_segments();

    // Verify termination
    assert_eq!(segments.len(), 1);
    let segment = &segments[0];

    // PCB should have 2 entries: core + child (terminated)
    assert_eq!(segment.pcb.len(), 2);

    // Last entry should be terminal
    let last_entry = segment.pcb.as_entries.last().unwrap();
    assert_eq!(last_entry.isd_as, child_as);
    assert_eq!(last_entry.next_isd_as, None); // Terminal
    assert_eq!(last_entry.hop_entry.egress, None); // Terminal
}

// ===== Up Segment Registration Tests (§4.1.2) =====

#[test]
fn test_up_segment_registration_non_core() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Insert PCBs in child's beacon store
    let cs = &mut net.scion_services.get_mut(&child_as).unwrap();

    for i in 0..5 {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000 + i, i as u16));
        pcb.extend(AsEntry::new(
            core_as,
            Some(child_as),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        cs.beacon_store.insert(Arc::new(pcb));
    }

    // Register up segments
    let segments = cs.register_up_segments();

    // Should register all 5 PCBs as up segments
    assert_eq!(segments.len(), 5);

    // All should be up segments
    for segment in &segments {
        assert_eq!(segment.segment_type, SegmentType::Up);
    }

    // Verify stored in local path database
    let up_segments = cs.path_db.get_up_to(core_as);
    assert_eq!(up_segments.len(), 5);
}

#[test]
fn test_up_segment_registration_core_as_does_nothing() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let core_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, core_as, true).unwrap(); // Core AS

    let cs = &mut net.scion_services.get_mut(&core_as).unwrap();

    // Try to register up segments (should do nothing for core AS)
    let segments = cs.register_up_segments();

    // Core ASes don't register up segments
    assert_eq!(segments.len(), 0);
}

// ===== Down Segment Registration Tests (§4.1.3) =====

#[test]
fn test_down_segment_registration_non_core() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Insert PCBs in child's beacon store
    let cs = &mut net.scion_services.get_mut(&child_as).unwrap();

    for i in 0..3 {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000 + i, i as u16));
        pcb.extend(AsEntry::new(
            core_as,
            Some(child_as),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        cs.beacon_store.insert(Arc::new(pcb));
    }

    // Register down segments
    let batches = cs.register_down_segments();

    // Should create one batch for the core AS
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].0, core_as);

    // Should have 3 segments
    let segments = &batches[0].1;
    assert_eq!(segments.len(), 3);

    // All should be down segments
    for segment in segments {
        assert_eq!(segment.segment_type, SegmentType::Down);
    }
}

#[test]
fn test_down_segment_registration_multiple_cores() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Two core ASes
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    let r3 = net.add_router("r3", 65102);
    net.add_link(r1, r2).unwrap();
    net.add_link(r2, r3).unwrap();

    let core1 = IsdAs::new(1, 100);
    let core2 = IsdAs::new(1, 101);
    let child_as = IsdAs::new(1, 102);

    net.enable_scion_router(r1, core1, true).unwrap();
    net.enable_scion_router(r2, core2, true).unwrap();
    net.enable_scion_router(r3, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();
    net.add_scion_link(r2, r3, ScionLinkType::Child).unwrap();

    // Insert PCBs from both cores in child's beacon store
    let cs = &mut net.scion_services.get_mut(&child_as).unwrap();

    // PCBs from core1
    for i in 0..2 {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000 + i, i as u16));
        pcb.extend(AsEntry::new(
            core1,
            Some(core2),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        pcb.extend(AsEntry::new(
            core2,
            Some(child_as),
            HopEntry::new(InterfaceId::new(1), Some(InterfaceId::new(2))),
        ));
        cs.beacon_store.insert(Arc::new(pcb));
    }

    // PCBs from core2
    for i in 2..4 {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000 + i, i as u16));
        pcb.extend(AsEntry::new(
            core2,
            Some(child_as),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(2))),
        ));
        cs.beacon_store.insert(Arc::new(pcb));
    }

    // Register down segments
    let batches = cs.register_down_segments();

    // Should create two batches (one per core)
    assert_eq!(batches.len(), 2);

    // Find batches for each core
    let batch1 = batches.iter().find(|(as_id, _)| *as_id == core1).unwrap();
    let batch2 = batches.iter().find(|(as_id, _)| *as_id == core2).unwrap();

    // Core1 should get 2 segments
    assert_eq!(batch1.1.len(), 2);

    // Core2 should get 2 segments
    assert_eq!(batch2.1.len(), 2);
}

#[test]
fn test_down_segment_registration_core_as_does_nothing() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let core_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, core_as, true).unwrap();

    let cs = &mut net.scion_services.get_mut(&core_as).unwrap();

    // Try to register down segments (should do nothing for core AS)
    let batches = cs.register_down_segments();

    // Core ASes don't register down segments
    assert_eq!(batches.len(), 0);
}

// ===== Core Segment Registration Tests (§4.2) =====

#[test]
fn test_core_segment_registration() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core1 = IsdAs::new(1, 100);
    let core2 = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core1, true).unwrap();
    net.enable_scion_router(r2, core2, true).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();

    // Insert PCBs in core1's beacon store
    let cs = &mut net.scion_services.get_mut(&core1).unwrap();

    for i in 0..3 {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000 + i, i as u16));
        pcb.extend(AsEntry::new(
            core2,
            Some(core1),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        cs.beacon_store.insert(Arc::new(pcb));
    }

    // Register core segments
    let segments = cs.register_core_segments();

    // Should register all 3 PCBs as core segments
    assert_eq!(segments.len(), 3);

    // All should be core segments
    for segment in &segments {
        assert_eq!(segment.segment_type, SegmentType::Core);
    }

    // Verify stored in local path database
    let core_segments = cs.path_db.get_core_from(core2);
    assert_eq!(core_segments.len(), 3);
}

#[test]
fn test_core_segment_registration_non_core_does_nothing() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let child_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, child_as, false).unwrap(); // Non-core

    let cs = &mut net.scion_services.get_mut(&child_as).unwrap();

    // Try to register core segments (should do nothing for non-core AS)
    let segments = cs.register_core_segments();

    // Non-core ASes don't register core segments
    assert_eq!(segments.len(), 0);
}

// ===== Down Segment Validation Tests (§4.1.3) =====

#[test]
fn test_down_segment_validation_accept_valid() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let core_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, core_as, true).unwrap();

    // Create valid down segment (originates from core_as)
    let mut pcb = Pcb::new(core_as);
    pcb.extend(AsEntry::new(
        core_as,
        Some(IsdAs::new(1, 101)),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));

    let segment = PathSegment::new(SegmentType::Down, pcb);

    let cs = &net.scion_services[&core_as];

    // Should accept (first AS equals core AS)
    assert!(cs.validate_down_segment(&segment));
}

#[test]
fn test_down_segment_validation_reject_invalid() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let core_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, core_as, true).unwrap();

    // Create invalid down segment (originates from different AS)
    let wrong_as = IsdAs::new(1, 101);
    let mut pcb = Pcb::new(wrong_as);
    pcb.extend(AsEntry::new(
        wrong_as,
        Some(IsdAs::new(1, 102)),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));

    let segment = PathSegment::new(SegmentType::Down, pcb);

    let cs = &net.scion_services[&core_as];

    // Should reject (first AS doesn't match core AS)
    assert!(!cs.validate_down_segment(&segment));
}

// ===== RegistrationTimeout Event Tests =====

#[test]
fn test_registration_timeout_non_core() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Insert PCBs in child's beacon store
    let cs = &mut net.scion_services.get_mut(&child_as).unwrap();

    let mut pcb = Pcb::new(core_as);
    pcb.extend(AsEntry::new(
        core_as,
        Some(child_as),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));
    cs.beacon_store.insert(Arc::new(pcb));

    // Trigger RegistrationTimeout
    let event = ScionEvent::RegistrationTimeout;
    let scion_event = Event::scion((), child_as, child_as, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    let (_, new_events) = result.unwrap();

    // Should create SegmentRegistration event for down segments
    assert_eq!(new_events.len(), 1);

    // Verify it's a SegmentRegistration event to core AS
    if let Event::Scion { src, dst, e, .. } = &new_events[0] {
        assert_eq!(*src, child_as);
        assert_eq!(*dst, core_as);
        if let ScionEvent::SegmentRegistration { segment_type, .. } = e {
            assert_eq!(*segment_type, SegmentType::Down);
        } else {
            panic!("Expected SegmentRegistration event");
        }
    } else {
        panic!("Expected SCION event");
    }

    // Verify up segments were stored locally
    let cs = &net.scion_services[&child_as];
    let up_segments = cs.path_db.get_up_to(core_as);
    assert_eq!(up_segments.len(), 1);
}

#[test]
fn test_registration_timeout_core() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core1 = IsdAs::new(1, 100);
    let core2 = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core1, true).unwrap();
    net.enable_scion_router(r2, core2, true).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();

    // Insert PCB in core1's beacon store
    let cs = &mut net.scion_services.get_mut(&core1).unwrap();

    let mut pcb = Pcb::new(core2);
    pcb.extend(AsEntry::new(
        core2,
        Some(core1),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));
    cs.beacon_store.insert(Arc::new(pcb));

    // Trigger RegistrationTimeout
    let event = ScionEvent::RegistrationTimeout;
    let scion_event = Event::scion((), core1, core1, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    let (_, new_events) = result.unwrap();

    // Core AS doesn't send SegmentRegistration events (local storage only)
    assert_eq!(new_events.len(), 0);

    // Verify core segments were stored locally
    let cs = &net.scion_services[&core1];
    let core_segments = cs.path_db.get_core_from(core2);
    assert_eq!(core_segments.len(), 1);
}

// ===== Full Registration Flow Tests =====

#[test]
fn test_full_registration_flow() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Topology: core -> child
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Step 1: Core originates PCB and sends to child
    let event = ScionEvent::BeaconTimeout {
        interval_type: BeaconIntervalType::IntraIsd,
    };
    let scion_event = Event::scion((), core_as, core_as, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    let (_, events) = result.unwrap();
    assert_eq!(events.len(), 1);

    // Step 2: Child receives PCB
    let result = net.handle_scion_event(events[0].clone());
    assert!(result.is_ok());

    // Step 3: Child registers segments
    let event = ScionEvent::RegistrationTimeout;
    let scion_event = Event::scion((), child_as, child_as, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    let (_, events) = result.unwrap();
    assert_eq!(events.len(), 1); // SegmentRegistration to core

    // Step 4: Core receives down segment
    let result = net.handle_scion_event(events[0].clone());
    assert!(result.is_ok());

    // Verify final state:

    // Child has up segment
    let child_cs = &net.scion_services[&child_as];
    let up_segments = child_cs.path_db.get_up_to(core_as);
    assert_eq!(up_segments.len(), 1);

    // Core has down segment
    let core_cs = &net.scion_services[&core_as];
    let down_segments = core_cs.path_db.get_down_to(child_as);
    assert_eq!(down_segments.len(), 1);

    // Verify segment termination
    let up_seg = &up_segments[0];
    assert_eq!(up_seg.pcb.len(), 2); // core + child (terminated)
    assert!(up_seg.pcb.is_terminated());

    let down_seg = &down_segments[0];
    assert_eq!(down_seg.pcb.len(), 2);
    assert!(down_seg.pcb.is_terminated());
}

#[test]
fn test_segment_registration_event_validation() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let core_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, core_as, true).unwrap();

    // Create invalid down segment (wrong origin)
    let wrong_as = IsdAs::new(1, 101);
    let mut pcb = Pcb::new(wrong_as);
    pcb.extend(AsEntry::new(
        wrong_as,
        Some(IsdAs::new(1, 102)),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));
    pcb.extend(AsEntry::new(
        IsdAs::new(1, 102),
        None, // Terminated
        HopEntry::new(InterfaceId::new(1), None),
    ));

    let segment = PathSegment::new(SegmentType::Down, pcb);

    // Send to core AS
    let event = ScionEvent::SegmentRegistration {
        segments: vec![Arc::new(segment)],
        segment_type: SegmentType::Down,
    };
    let scion_event = Event::scion((), wrong_as, core_as, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    // Segment should be rejected (not stored)
    let cs = &net.scion_services[&core_as];
    assert_eq!(cs.path_db.get_all().len(), 0);
}
