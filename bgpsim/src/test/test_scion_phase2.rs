// Phase 2 Integration Tests: SCION Event System
//
// These tests verify the event-driven infrastructure for SCION beaconing:
// - Network setup with SCION-enabled routers
// - SCION link creation
// - Event handling (BeaconBatch, SegmentRegistration)
// - State verification (beacon store, path database)

use crate::event::Event;
use crate::network::Network;
use crate::scion::*;
use crate::types::SimplePrefix;
use std::sync::Arc;

#[test]
fn test_enable_scion_router() {
    // Create a simple network
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Add routers
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65100);
    net.add_link(r1, r2).unwrap();

    // Enable SCION on routers
    let isd_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, isd_as, true).unwrap();
    net.enable_scion_router(r2, isd_as, true).unwrap();

    // Verify control service was created
    assert!(net.scion_services.contains_key(&isd_as));
    let cs = &net.scion_services[&isd_as];
    assert_eq!(cs.isd_as, isd_as);
    assert!(cs.is_core);

    // Verify routers are mapped
    assert_eq!(net.router_to_as.get(&r1), Some(&isd_as));
    assert_eq!(net.router_to_as.get(&r2), Some(&isd_as));
}

#[test]
fn test_add_scion_link() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Create two ASes
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let as1 = IsdAs::new(1, 100);
    let as2 = IsdAs::new(1, 101);

    net.enable_scion_router(r1, as1, true).unwrap();
    net.enable_scion_router(r2, as2, false).unwrap();

    // Add SCION link (parent-child)
    net.add_scion_link(r1, r2, ScionLinkType::Child).unwrap();

    // Verify interfaces were created
    let cs1 = &net.scion_services[&as1];
    let cs2 = &net.scion_services[&as2];

    assert_eq!(cs1.interfaces.len(), 1);
    assert_eq!(cs2.interfaces.len(), 1);

    // Verify border routers
    assert!(cs1.is_border_router(r1));
    assert!(cs2.is_border_router(r2));

    // Verify link types
    let iface1 = InterfaceId::new(1);
    let iface2 = InterfaceId::new(1);

    let info1 = &cs1.interfaces[&iface1];
    assert_eq!(info1.link_type, ScionLinkType::Child);
    assert_eq!(info1.remote_as, as2);
    assert_eq!(info1.local_router, r1);

    let info2 = &cs2.interfaces[&iface2];
    assert_eq!(info2.link_type, ScionLinkType::Parent);
    assert_eq!(info2.remote_as, as1);
    assert_eq!(info2.local_router, r2);
}

#[test]
fn test_handle_beacon_batch_event() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Setup two core ASes
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65101);
    net.add_link(r1, r2).unwrap();

    let as1 = IsdAs::new(1, 100);
    let as2 = IsdAs::new(1, 101);

    net.enable_scion_router(r1, as1, true).unwrap(); // Core AS
    net.enable_scion_router(r2, as2, true).unwrap(); // Core AS

    // Add SCION link
    net.add_scion_link(r1, r2, ScionLinkType::Core).unwrap();

    // Create a PCB from AS1 (not containing AS2)
    let mut pcb = Pcb::new(as1);
    let entry = AsEntry::new(
        as1,
        Some(as2), // Next AS is AS2
        HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
    );
    pcb.extend(entry);
    let pcb = Arc::new(pcb);

    // Create BeaconBatch event (AS1 sending to AS2)
    let event = ScionEvent::BeaconBatch {
        pcbs: vec![pcb.clone()],
        link_type: ScionLinkType::Core,
    };

    let scion_event = Event::scion((), as1, as2, event);

    // Process event
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    // Verify PCB was stored in AS2
    let cs = &net.scion_services[&as2];
    let stored_pcbs: Vec<_> = cs.beacon_store.get_all().collect();
    assert_eq!(stored_pcbs.len(), 1);

    // Verify it's the same PCB (Arc pointer comparison)
    assert!(Arc::ptr_eq(&stored_pcbs[0], &pcb));
}

#[test]
fn test_handle_segment_registration_event() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Setup network
    let r1 = net.add_router("r1", 65100);
    let isd_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, isd_as, true).unwrap();

    // Create a path segment
    let mut pcb = Pcb::new(isd_as);
    let entry = AsEntry::new(
        isd_as,
        None,
        HopEntry::new(InterfaceId::ZERO, None),
    );
    pcb.extend(entry);

    let segment = PathSegment::new(SegmentType::Up, pcb);
    let segment = Arc::new(segment);

    // Create SegmentRegistration event
    let event = ScionEvent::SegmentRegistration {
        segments: vec![segment.clone()],
        segment_type: SegmentType::Up,
    };

    let scion_event = Event::scion((), isd_as, isd_as, event);

    // Process event
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_ok());

    // Verify segment was stored
    let cs = &net.scion_services[&isd_as];
    let stored_segments = cs.path_db.get_all();
    assert_eq!(stored_segments.len(), 1);

    // Verify it's the same segment (Arc pointer comparison)
    assert!(Arc::ptr_eq(&stored_segments[0], &segment));
}

#[test]
fn test_scion_link_validation() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Create two routers in the SAME AS
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65100);
    net.add_link(r1, r2).unwrap();

    let isd_as = IsdAs::new(1, 100);
    net.enable_scion_router(r1, isd_as, true).unwrap();
    net.enable_scion_router(r2, isd_as, true).unwrap();

    // Try to add SCION link between routers in same AS (should fail)
    let result = net.add_scion_link(r1, r2, ScionLinkType::Core);
    assert!(result.is_err());

    // Verify error message
    if let Err(e) = result {
        assert!(e.to_string().contains("same AS"));
    }
}

#[test]
fn test_multiple_scion_links_same_as() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Core AS with two border routers
    let r1 = net.add_router("r1", 65100);
    let r2 = net.add_router("r2", 65100);
    net.add_link(r1, r2).unwrap();

    // Child AS with two border routers
    let r3 = net.add_router("r3", 65101);
    let r4 = net.add_router("r4", 65101);
    net.add_link(r3, r4).unwrap();

    // Add physical links
    net.add_link(r1, r3).unwrap();
    net.add_link(r2, r4).unwrap();

    let as1 = IsdAs::new(1, 100);
    let as2 = IsdAs::new(1, 101);

    net.enable_scion_router(r1, as1, true).unwrap();
    net.enable_scion_router(r2, as1, true).unwrap();
    net.enable_scion_router(r3, as2, false).unwrap();
    net.enable_scion_router(r4, as2, false).unwrap();

    // Add two SCION links between same AS pair
    net.add_scion_link(r1, r3, ScionLinkType::Child).unwrap();
    net.add_scion_link(r2, r4, ScionLinkType::Child).unwrap();

    // Verify both ASes have 2 interfaces
    let cs1 = &net.scion_services[&as1];
    let cs2 = &net.scion_services[&as2];

    assert_eq!(cs1.interfaces.len(), 2);
    assert_eq!(cs2.interfaces.len(), 2);

    // Verify both routers are border routers
    assert!(cs1.is_border_router(r1));
    assert!(cs1.is_border_router(r2));
    assert!(cs2.is_border_router(r3));
    assert!(cs2.is_border_router(r4));
}

#[test]
fn test_scion_event_nonexistent_as() {
    let mut net: Network<SimplePrefix, _> = Network::default();

    // Create event for AS that doesn't exist
    let nonexistent_as = IsdAs::new(99, 999);
    let event = ScionEvent::BeaconBatch {
        pcbs: vec![],
        link_type: ScionLinkType::Core,
    };

    let scion_event = Event::scion((), nonexistent_as, nonexistent_as, event);

    // Should fail with appropriate error
    let result = net.handle_scion_event(scion_event);
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(e.to_string().contains("not found"));
    }
}
