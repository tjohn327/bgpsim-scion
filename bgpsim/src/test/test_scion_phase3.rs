// Phase 3 Tests: PCB Propagation Logic
//
// These tests verify the SCION beaconing implementation:
// - PCB validation (loop detection, interface validation, continuity)
// - PCB selection (shortest path, neighbor filtering)
// - PCB extension (intra-ISD and core)
// - PCB propagation (create BeaconBatch events)
// - Full beaconing scenarios

use crate::event::Event;
use crate::network::Network;
use crate::scion::*;
use crate::types::SimplePrefix;
use std::sync::Arc;

// ===== PCB Validation Tests (§2.3.1) =====

#[test]
fn test_pcb_validation_loop_detection() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let as1 = IsdAs::new(1, 100);
    net.enable_scion_router(r1, as1, true).unwrap();

    // Create a PCB that already contains AS1
    let mut pcb = Pcb::new(as1);
    let entry = AsEntry::new(
        as1,
        Some(IsdAs::new(1, 101)),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    );
    pcb.extend(entry);

    // Send to AS1 (which is already in the PCB)
    let event = ScionEvent::BeaconBatch {
        pcbs: vec![Arc::new(pcb)],
        link_type: ScionLinkType::Core,
    };

    let scion_event = Event::scion((), as1, as1, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    // PCB should be rejected due to loop
    let cs = &net.scion_services[&as1];
    assert_eq!(cs.beacon_store.get_all().count(), 0);
}

#[test]
fn test_pcb_validation_continuity_check() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let as1 = IsdAs::new(1, 100);
    let as2 = IsdAs::new(1, 101);
    let as3 = IsdAs::new(1, 102);
    let as4 = IsdAs::new(1, 103); // Not in continuity chain

    net.enable_scion_router(r1, as1, true).unwrap();
    net.enable_scion_router(r2, as2, true).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();

    // Create a PCB with continuity violation
    let mut pcb = Pcb::new(as1);

    // Entry 1: AS1 -> AS2 (correct)
    pcb.extend(AsEntry::new(
        as1,
        Some(as2),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));

    // Entry 2: AS3 -> AS4 (WRONG! Should be AS2)
    pcb.extend(AsEntry::new(
        as3,
        Some(as4),
        HopEntry::new(InterfaceId::new(1), Some(InterfaceId::new(2))),
    ));

    // Send to AS2
    let event = ScionEvent::BeaconBatch {
        pcbs: vec![Arc::new(pcb)],
        link_type: ScionLinkType::Core,
    };

    let scion_event = Event::scion((), as1, as2, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    // PCB should be rejected due to continuity violation
    let cs = &net.scion_services[&as2];
    assert_eq!(cs.beacon_store.get_all().count(), 0);
}

#[test]
fn test_pcb_validation_link_type() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Non-core AS
    let r1 = net.add_router("r1", 65100);
    let as1 = IsdAs::new(1, 100);
    net.enable_scion_router(r1, as1, false).unwrap(); // Non-core

    // Create a valid PCB
    let mut pcb = Pcb::new(IsdAs::new(1, 99));
    pcb.extend(AsEntry::new(
        IsdAs::new(1, 99),
        Some(as1),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));

    // Try to send with wrong link type (Core to non-core AS)
    let event = ScionEvent::BeaconBatch {
        pcbs: vec![Arc::new(pcb)],
        link_type: ScionLinkType::Core, // Wrong! Non-core should receive Parent
    };

    let scion_event = Event::scion((), IsdAs::new(1, 99), as1, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    // PCB should be rejected due to wrong link type
    let cs = &net.scion_services[&as1];
    assert_eq!(cs.beacon_store.get_all().count(), 0);
}

// ===== PCB Selection Tests (§2.3.3) =====

#[test]
fn test_pcb_selection_shortest_path() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let as1 = IsdAs::new(1, 100);
    net.enable_scion_router(r1, as1, true).unwrap();

    let cs = &mut net.scion_services.get_mut(&as1).unwrap();

    // Create PCBs of different lengths
    let mut pcb_short = Pcb::new(IsdAs::new(1, 99));
    pcb_short.extend(AsEntry::new(
        IsdAs::new(1, 99),
        Some(as1),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));

    let mut pcb_long = Pcb::new(IsdAs::new(1, 98));
    pcb_long.extend(AsEntry::new(
        IsdAs::new(1, 98),
        Some(IsdAs::new(1, 99)),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));
    pcb_long.extend(AsEntry::new(
        IsdAs::new(1, 99),
        Some(as1),
        HopEntry::new(InterfaceId::new(1), Some(InterfaceId::new(2))),
    ));

    // Insert in reverse order (long first)
    cs.beacon_store.insert(Arc::new(pcb_long.clone()));
    cs.beacon_store.insert(Arc::new(pcb_short.clone()));

    // Select best PCB
    let selected = cs.select_best_pcbs(1);
    assert_eq!(selected.len(), 1);

    // Should select the shorter PCB
    assert_eq!(selected[0].len(), 1);
}

#[test]
fn test_pcb_selection_for_neighbor() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let as1 = IsdAs::new(1, 100);
    net.enable_scion_router(r1, as1, true).unwrap();

    let neighbor = IsdAs::new(1, 101);
    let cs = &mut net.scion_services.get_mut(&as1).unwrap();

    // Create PCB that contains neighbor (should be filtered out)
    let mut pcb_with_neighbor = Pcb::with_segment_info(SegmentInfo::with_values(1000, 1));
    pcb_with_neighbor.extend(AsEntry::new(
        IsdAs::new(1, 99),
        Some(neighbor),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));
    pcb_with_neighbor.extend(AsEntry::new(
        neighbor,
        Some(as1),
        HopEntry::new(InterfaceId::new(1), Some(InterfaceId::new(2))),
    ));

    // Create PCB that doesn't contain neighbor (should be selected)
    let mut pcb_without_neighbor = Pcb::with_segment_info(SegmentInfo::with_values(1000, 2));
    pcb_without_neighbor.extend(AsEntry::new(
        IsdAs::new(1, 99),
        Some(as1),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));

    cs.beacon_store.insert(Arc::new(pcb_with_neighbor));
    cs.beacon_store.insert(Arc::new(pcb_without_neighbor));

    // Select for neighbor
    let selected = cs.select_best_pcbs_for_neighbor(neighbor, 10);

    // Should only select PCB without neighbor
    assert_eq!(selected.len(), 1);
    assert!(!selected[0].contains(neighbor));
}

// ===== PCB Extension Tests (§2.3.5) =====

#[test]
fn test_pcb_extension_intra_isd() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Core AS with child
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Create a PCB
    let mut pcb = Pcb::new(core_as);
    pcb.extend(AsEntry::new(
        core_as,
        Some(child_as),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));
    let pcb = Arc::new(pcb);

    // Extend PCB at child AS
    let cs = &net.scion_services[&child_as];
    let iface_id = InterfaceId::new(1);
    let next_as = IsdAs::new(1, 102);

    let extended = cs.extend_pcb_intra_isd(pcb.clone(), iface_id, next_as);

    // Verify extension
    assert_eq!(extended.len(), 2);
    let last_entry = &extended.as_entries[1];
    assert_eq!(last_entry.isd_as, child_as);
    assert_eq!(last_entry.next_isd_as, Some(next_as));
    assert_eq!(last_entry.hop_entry.egress, Some(iface_id));
}

#[test]
fn test_pcb_origination() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Originate PCB
    let cs = &net.scion_services[&core_as];
    let egress_iface = InterfaceId::new(1);

    let pcb = cs.originate_pcb(egress_iface, child_as);

    // Verify PCB
    assert_eq!(pcb.len(), 1);
    let entry = &pcb.as_entries[0];
    assert_eq!(entry.isd_as, core_as);
    assert_eq!(entry.next_isd_as, Some(child_as));
    assert_eq!(entry.hop_entry.ingress, InterfaceId::ZERO); // Origin has no ingress
    assert_eq!(entry.hop_entry.egress, Some(egress_iface));
}

// ===== PCB Propagation Tests =====

#[test]
fn test_propagation_intra_isd_core_origination() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Core AS with two children
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    let r3 = net.add_router("r3", 65102);
    net.add_link(r1, r2).unwrap();
    net.add_link(r1, r3).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child1 = IsdAs::new(1, 101);
    let child2 = IsdAs::new(1, 102);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child1, false).unwrap();
    net.enable_scion_router(r3, child2, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();
    net.add_scion_link(r1, r3, ScionLinkType::Child).unwrap();

    // Propagate intra-ISD PCBs
    let cs = &mut net.scion_services.get_mut(&core_as).unwrap();
    let batches = cs.propagate_intra_isd_pcbs();

    // Should create PCBs for both children
    assert_eq!(batches.len(), 2);

    // Each batch should have one PCB
    for (_, pcbs) in &batches {
        assert_eq!(pcbs.len(), 1);
        assert_eq!(pcbs[0].len(), 1); // One AS entry (core AS)
    }
}

#[test]
fn test_propagation_intra_isd_forwarding() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Three-level hierarchy: core -> child -> grandchild
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    let r3 = net.add_router("r3", 65102);
    net.add_link(r1, r2).unwrap();
    net.add_link(r2, r3).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);
    let grandchild_as = IsdAs::new(1, 102);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.enable_scion_router(r3, grandchild_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();
    net.add_scion_link(r2, r3, ScionLinkType::Child).unwrap();

    // Insert a PCB from core in child's beacon store
    let mut pcb = Pcb::new(core_as);
    pcb.extend(AsEntry::new(
        core_as,
        Some(child_as),
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    ));

    let cs = &mut net.scion_services.get_mut(&child_as).unwrap();
    cs.beacon_store.insert(Arc::new(pcb));

    // Propagate from child to grandchild
    let batches = cs.propagate_intra_isd_pcbs();

    // Should create one batch for grandchild
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].0, grandchild_as);

    // PCB should be extended with child's AS entry
    let extended_pcb = &batches[0].1[0];
    assert_eq!(extended_pcb.len(), 2); // Core + Child
}

#[test]
fn test_propagation_core_beaconing() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Three core ASes in a line
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    let r3 = net.add_router("r3", 65102);
    net.add_link(r1, r2).unwrap();
    net.add_link(r2, r3).unwrap();

    let core1 = IsdAs::new(1, 100);
    let core2 = IsdAs::new(1, 101);
    let core3 = IsdAs::new(1, 102);

    net.enable_scion_router(r1, core1, true).unwrap();
    net.enable_scion_router(r2, core2, true).unwrap();
    net.enable_scion_router(r3, core3, true).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();
    net.add_scion_link(r2, r3, ScionLinkType::Core).unwrap();

    // Core2 propagates to neighbors
    let cs = &mut net.scion_services.get_mut(&core2).unwrap();
    let batches = cs.propagate_core_pcbs();

    // Should create PCBs for both neighbors (core1 and core3)
    assert_eq!(batches.len(), 2);
}

// ===== Full Beaconing Scenario Tests =====

#[test]
fn test_full_intra_isd_beaconing() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Topology: core -> child -> grandchild
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    let r3 = net.add_router("r3", 65102);
    net.add_link(r1, r2).unwrap();
    net.add_link(r2, r3).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);
    let grandchild_as = IsdAs::new(1, 102);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.enable_scion_router(r3, grandchild_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();
    net.add_scion_link(r2, r3, ScionLinkType::Child).unwrap();

    // Step 1: Core AS timeout (originate PCBs)
    let event = ScionEvent::BeaconTimeout {
        interval_type: BeaconIntervalType::IntraIsd,
    };
    let scion_event = Event::scion((), core_as, core_as, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    let (_, new_events) = result.unwrap();
    assert_eq!(new_events.len(), 1); // One BeaconBatch to child

    // Step 2: Process BeaconBatch at child
    let result = net.handle_scion_event(new_events[0].clone());
    assert!(result.is_ok());

    // Verify child received PCB
    let cs = &net.scion_services[&child_as];
    assert_eq!(cs.beacon_store.get_all().count(), 1);

    // Step 3: Child AS timeout (forward PCBs)
    let event = ScionEvent::BeaconTimeout {
        interval_type: BeaconIntervalType::IntraIsd,
    };
    let scion_event = Event::scion((), child_as, child_as, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    let (_, new_events) = result.unwrap();
    assert_eq!(new_events.len(), 1); // One BeaconBatch to grandchild

    // Step 4: Process BeaconBatch at grandchild
    let result = net.handle_scion_event(new_events[0].clone());
    assert!(result.is_ok());

    // Verify grandchild received extended PCB
    let cs = &net.scion_services[&grandchild_as];
    let stored_pcbs: Vec<_> = cs.beacon_store.get_all().collect();
    assert_eq!(stored_pcbs.len(), 1);
    assert_eq!(stored_pcbs[0].len(), 2); // Core + Child
}

#[test]
fn test_full_core_beaconing() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Three core ASes
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    let r3 = net.add_router("r3", 65102);
    net.add_link(r1, r2).unwrap();
    net.add_link(r2, r3).unwrap();

    let core1 = IsdAs::new(1, 100);
    let core2 = IsdAs::new(1, 101);
    let core3 = IsdAs::new(1, 102);

    net.enable_scion_router(r1, core1, true).unwrap();
    net.enable_scion_router(r2, core2, true).unwrap();
    net.enable_scion_router(r3, core3, true).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();
    net.add_scion_link(r2, r3, ScionLinkType::Core).unwrap();

    // Core1 initiates beaconing
    let event = ScionEvent::BeaconTimeout {
        interval_type: BeaconIntervalType::Core,
    };
    let scion_event = Event::scion((), core1, core1, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    let (_, events) = result.unwrap();
    assert_eq!(events.len(), 1); // Beacon to core2

    // Core2 receives beacon
    let result = net.handle_scion_event(events[0].clone());
    assert!(result.is_ok());

    // Verify core2 received PCB
    let cs = &net.scion_services[&core2];
    assert_eq!(cs.beacon_store.get_all().count(), 1);

    // Core2 propagates
    let event = ScionEvent::BeaconTimeout {
        interval_type: BeaconIntervalType::Core,
    };
    let scion_event = Event::scion((), core2, core2, event);
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    let (_, events) = result.unwrap();
    // Should propagate to both neighbors (core1 and core3)
    // But core1 will be filtered out due to loop detection
    assert!(events.len() >= 1);
}

#[test]
fn test_pcb_limit_enforcement_intra_isd() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core_as = IsdAs::new(1, 100);
    let child_as = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core_as, true).unwrap();
    net.enable_scion_router(r2, child_as, false).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Insert 100 PCBs in child's beacon store
    let cs = &mut net.scion_services.get_mut(&child_as).unwrap();
    for i in 0..100 {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000 + i, i as u16));
        pcb.extend(AsEntry::new(
            core_as,
            Some(child_as),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        cs.beacon_store.insert(Arc::new(pcb));
    }

    // Propagate
    let batches = cs.propagate_intra_isd_pcbs();

    // Should limit to ≤50 PCBs per child (§3.4.1)
    for (_, pcbs) in batches {
        assert!(pcbs.len() <= 50);
    }
}

#[test]
fn test_pcb_limit_enforcement_core() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let core1 = IsdAs::new(1, 100);
    let core2 = IsdAs::new(1, 101);

    net.enable_scion_router(r1, core1, true).unwrap();
    net.enable_scion_router(r2, core2, true).unwrap();
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();

    // Insert 20 PCBs in core1's beacon store
    let cs = &mut net.scion_services.get_mut(&core1).unwrap();
    for i in 0..20 {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000 + i, i as u16));
        pcb.extend(AsEntry::new(
            IsdAs::new(1, 99),
            Some(core1),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        cs.beacon_store.insert(Arc::new(pcb));
    }

    // Propagate
    let batches = cs.propagate_core_pcbs();

    // Should limit to ≤5 PCBs per neighbor (§3.4.2)
    for (_, pcbs) in batches {
        assert!(pcbs.len() <= 5);
    }
}
