//! SCION Event-Driven Simulation Example
//!
//! This example demonstrates step-by-step event processing in SCION simulation.
//! Instead of using batch APIs, we manually process SCION events to see the
//! detailed flow of beacon propagation.
//!
//! This approach is useful for:
//! - Debugging beacon propagation
//! - Understanding the control plane protocol
//! - Implementing custom timing models
//! - Analyzing intermediate states during beaconing
//!
//! Run with: cargo run --example scion_event_driven --all-features

use bgpsim::event::BasicEventQueue;
use bgpsim::network::Network;
use bgpsim::ospf::MinimalOspf;
use bgpsim::scion::*;
use bgpsim::types::{NetworkError, SimplePrefix};
use std::sync::Arc;

type ScionNetwork = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, MinimalOspf>;

fn main() -> Result<(), NetworkError> {
    println!("=== SCION Event-Driven Simulation ===\n");

    // Create a simple 3-AS topology
    let mut net: ScionNetwork = Network::default();

    // AS identifiers
    let core = IsdAs::new(1, 100);
    let provider = IsdAs::new(1, 200);
    let leaf = IsdAs::new(1, 300);

    // Create routers
    let r_core = net.add_router("core_br", core.asn.0);
    let r_provider = net.add_router("provider_br", provider.asn.0);
    let r_leaf = net.add_router("leaf_br", leaf.asn.0);

    // Enable SCION
    net.enable_scion_router(r_core, core, true)?;
    net.enable_scion_router(r_provider, provider, false)?;
    net.enable_scion_router(r_leaf, leaf, false)?;

    // Add links
    net.add_link(r_core, r_provider)?;
    net.add_link(r_provider, r_leaf)?;

    // Add SCION links (parent-child relationships)
    net.add_scion_link(r_core, r_provider, ScionLinkType::Child)?;
    net.add_scion_link(r_provider, r_leaf, ScionLinkType::Child)?;

    println!("Topology: {} -> {} -> {}", core, provider, leaf);
    println!("          (core)  (provider)  (leaf)\n");

    // ========================================
    // PHASE 1: Manual PCB Origination at Core
    // ========================================
    println!("Phase 1: Core originates PCB");
    println!("─────────────────────────────");

    // Get the core's control service and manually originate a PCB
    let core_cs = net.get_scion_service(&core).unwrap();

    // Find the child interface (to provider)
    let child_iface = core_cs.interfaces.iter()
        .find(|(_, info)| info.link_type == ScionLinkType::Child)
        .map(|(id, _)| *id)
        .expect("Core should have child interface");

    println!("  Core interface {} connects to provider", child_iface.0);

    // The core creates a fresh PCB (origination)
    let segment_info = SegmentInfo::with_values(1000, 42); // timestamp, segment_id
    let mut pcb = Pcb::with_segment_info(segment_info);

    // Core adds its AS entry to the PCB
    let hop_entry = HopEntry::new(InterfaceId::ZERO, Some(child_iface));
    let as_entry = AsEntry::new(core, Some(provider), hop_entry);
    pcb.extend(as_entry);

    println!("  Created PCB with {} AS entry", pcb.as_entries.len());
    println!("    AS path: {}", format_as_path(&pcb));

    // ========================================
    // PHASE 2: Manual Event Creation & Delivery
    // ========================================
    println!("\nPhase 2: Beacon propagation (event-driven)");
    println!("───────────────────────────────────────────");

    // Create beacon event from core -> provider
    let _beacon_event = ScionEvent::beacon_batch(
        vec![Arc::new(pcb.clone())],
        ScionLinkType::Child,
    );

    println!("  Event: BeaconBatch from {} to {}", core, provider);

    // Simulate event delivery by directly calling the control service
    // In a full simulation, this would go through the event queue

    // Provider receives the PCB and processes it
    let provider_cs = net.get_scion_service(&provider).unwrap();

    // Find provider's interfaces
    let provider_parent_iface = provider_cs.interfaces.iter()
        .find(|(_, info)| info.link_type == ScionLinkType::Parent)
        .map(|(id, _)| *id)
        .expect("Provider should have parent interface");

    let provider_child_iface = provider_cs.interfaces.iter()
        .find(|(_, info)| info.link_type == ScionLinkType::Child)
        .map(|(id, _)| *id)
        .expect("Provider should have child interface");

    println!("  Provider interfaces: parent={}, child={}",
             provider_parent_iface.0, provider_child_iface.0);

    // Provider extends the PCB with its own AS entry
    let mut extended_pcb = pcb.clone();
    let provider_hop = HopEntry::new(provider_parent_iface, Some(provider_child_iface));
    let provider_entry = AsEntry::new(provider, Some(leaf), provider_hop);
    extended_pcb.extend(provider_entry);

    println!("  Provider extended PCB:");
    println!("    AS path: {}", format_as_path(&extended_pcb));
    println!("    Length: {} AS entries", extended_pcb.as_entries.len());

    // ========================================
    // PHASE 3: Final Delivery to Leaf
    // ========================================
    println!("\nPhase 3: Final beacon delivery");
    println!("───────────────────────────────");

    let _beacon_to_leaf = ScionEvent::beacon_batch(
        vec![Arc::new(extended_pcb.clone())],
        ScionLinkType::Child,
    );

    println!("  Event: BeaconBatch from {} to {}", provider, leaf);

    // Leaf receives and stores the PCB (it's a leaf, so it terminates)
    let leaf_cs = net.get_scion_service(&leaf).unwrap();
    let leaf_parent_iface = leaf_cs.interfaces.iter()
        .find(|(_, info)| info.link_type == ScionLinkType::Parent)
        .map(|(id, _)| *id)
        .expect("Leaf should have parent interface");

    // Leaf extends with final AS entry (egress = None for termination)
    let mut final_pcb = extended_pcb.clone();
    let leaf_hop = HopEntry::new(leaf_parent_iface, None);
    let leaf_entry = AsEntry::new(leaf, None, leaf_hop);
    final_pcb.extend(leaf_entry);

    println!("  Leaf terminates PCB:");
    println!("    Final AS path: {}", format_as_path(&final_pcb));
    println!("    Total hops: {}", final_pcb.as_entries.len());

    // ========================================
    // PHASE 4: Path Segment Creation
    // ========================================
    println!("\nPhase 4: Path segment registration");
    println!("───────────────────────────────────");

    // Leaf creates an UP segment from the terminated PCB
    let up_segment = PathSegment::new(SegmentType::Up, final_pcb.clone());
    println!("  Created UP segment: {} -> {}", leaf, core);
    println!("    Segment type: {:?}", up_segment.segment_type);

    // The same PCB, when viewed from core's perspective, is a DOWN segment
    let _down_segment = PathSegment::new(SegmentType::Down, final_pcb.clone());
    println!("  Created DOWN segment: {} -> {}", core, leaf);

    // ========================================
    // Summary: Event Types in SCION
    // ========================================
    println!("\n═══════════════════════════════════════════");
    println!("SCION Event Types Summary:");
    println!("═══════════════════════════════════════════");
    println!("  1. BeaconBatch     - PCB propagation between ASes");
    println!("  2. BeaconTimeout   - Triggers periodic beaconing");
    println!("  3. SegmentReg      - Register segments with core ASes");
    println!("  4. RegTimeout      - Triggers periodic registration");
    println!();
    println!("Event-driven simulation allows:");
    println!("  • Fine-grained control over timing");
    println!("  • Inspection of intermediate states");
    println!("  • Custom event ordering/prioritization");
    println!("  • Debugging complex multi-AS scenarios");

    println!("\n=== Example Complete ===");
    Ok(())
}

/// Format the AS path from a PCB
fn format_as_path(pcb: &Pcb) -> String {
    pcb.as_entries
        .iter()
        .map(|e| e.isd_as.to_string())
        .collect::<Vec<_>>()
        .join(" -> ")
}
