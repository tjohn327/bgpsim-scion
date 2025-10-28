# SCION User Guide for bgpsim

**Version**: 1.0
**Date**: 2025-10-28
**Status**: Complete Implementation

## Table of Contents

1. [Introduction](#introduction)
2. [Quick Start](#quick-start)
3. [Core Concepts](#core-concepts)
4. [Basic Usage](#basic-usage)
5. [Advanced Topics](#advanced-topics)
6. [Examples](#examples)
7. [API Reference](#api-reference)
8. [Performance Tuning](#performance-tuning)
9. [Troubleshooting](#troubleshooting)

## Introduction

bgpsim now supports SCION (Scalability, Control, and Isolation On Next-generation networks), a path-aware inter-domain routing architecture. This guide will help you understand and use SCION simulation in bgpsim.

### What is SCION?

SCION is a next-generation internet architecture that provides:
- **Path awareness**: Endhosts can see and select from multiple paths
- **Path control**: Applications choose paths based on their requirements
- **Isolation**: ISDs (Isolation Domains) provide fault isolation
- **Security**: Cryptographic path validation (simulated in bgpsim)

### Why Simulate SCION?

- **Research**: Study path diversity, convergence, and failure recovery
- **Comparison**: Compare SCION with BGP in the same simulation
- **Education**: Learn SCION concepts through hands-on simulation
- **Protocol design**: Test new path selection policies

## Quick Start

### Basic Example: Intra-ISD Network

```rust
use bgpsim::prelude::*;
use bgpsim::event::BasicEventQueue;
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::{IsdAs, ScionLinkType};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create network
    let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

    // Add routers
    let core = net.add_router("Core", 65500);
    let leaf1 = net.add_router("Leaf1", 65501);
    let leaf2 = net.add_router("Leaf2", 65502);

    // Add links
    net.add_link(core, leaf1)?;
    net.add_link(core, leaf2)?;

    // Enable SCION
    net.enable_scion(core, IsdAs::new(1, 110), true)?;  // Core AS
    net.enable_scion(leaf1, IsdAs::new(1, 111), false)?; // Non-core AS
    net.enable_scion(leaf2, IsdAs::new(1, 112), false)?; // Non-core AS

    // Configure links
    net.configure_scion_link(core, leaf1, ScionLinkType::ParentChild)?;
    net.configure_scion_link(core, leaf2, ScionLinkType::ParentChild)?;

    // Run SCION protocols
    net.scion_core_beaconing(1000)?;           // Core beaconing at time 1000
    net.scion_intra_isd_beaconing(1000, 50)?;  // Intra-ISD beaconing (50 best PCBs - spec default)
    net.scion_registration_round(50)?;         // Register segments (50 best - spec default)

    // Lookup paths
    let paths = net.scion_lookup_paths(leaf1, leaf2)?;

    println!("Found {} paths from Leaf1 to Leaf2:", paths.len());
    for (i, path) in paths.iter().enumerate() {
        println!("  Path {}: {:?}", i + 1, path.as_path);
        println!("    MTU: {}, Hops: {}", path.mtu, path.as_path.len());
    }

    Ok(())
}
```

## Core Concepts

### Isolation Domains (ISDs)

ISDs are the top-level grouping in SCION. They provide:
- Fault isolation between regions
- Independent trust roots
- Scalable routing

**In bgpsim**: ISDs are identified by 16-bit numbers (e.g., ISD 1, ISD 2)

### AS Numbers

SCION uses 48-bit AS numbers, allowing for much larger address space than BGP.

**In bgpsim**: Create with `IsdAs::new(isd, asn)`
```rust
let isd_as = IsdAs::new(1, 110);  // ISD 1, AS 110
```

### Link Types

SCION has three types of inter-AS links:

1. **Core**: Between core ASes (forms the core topology)
2. **Parent-Child**: Hierarchical relationship (provider-customer)
3. **Peering**: Lateral relationship between non-core ASes

**In bgpsim**:
```rust
// Core link
net.configure_scion_link(core1, core2, ScionLinkType::Core)?;

// Parent-child link
net.configure_scion_link(parent, child, ScionLinkType::ParentChild)?;

// Peering link
net.configure_scion_link(peer1, peer2, ScionLinkType::Peering)?;
```

### Path Segments

SCION paths are composed of segments:

- **Up-segment**: From non-core AS to core AS
- **Down-segment**: From core AS to non-core AS
- **Core-segment**: Between core ASes

**Path combinations**:
- Intra-ISD: Up + Down
- Inter-ISD: Up + Core + Down
- With shortcuts: Peering links can create shortcuts

### Beaconing Process

SCION uses periodic beaconing to disseminate path information:

1. **Core Beaconing**: Core ASes create and exchange PCBs (Path Construction Beacons)
2. **Intra-ISD Beaconing**: Core ASes propagate PCBs to child ASes
3. **Registration**: ASes register path segments at core ASes

**In bgpsim**:
```rust
// Step 1: Core beaconing
net.scion_core_beaconing(timestamp)?;

// Step 2: Intra-ISD beaconing (propagate up to 5 best PCBs)
net.scion_intra_isd_beaconing(timestamp, 50)?;

// Step 3: Registration (register up to 5 best segments)
net.scion_registration_round(50)?;

// Or all in one:
let (up_count, down_count, core_count) = net.scion_registration_round(50)?;
```

## Basic Usage

### 1. Enable SCION on Routers

```rust
// Core AS (participates in core beaconing)
net.enable_scion(router_id, IsdAs::new(1, 110), true)?;

// Non-core AS
net.enable_scion(router_id, IsdAs::new(1, 111), false)?;
```

### 2. Configure Links

```rust
// Core link (between core ASes, can be in different ISDs)
net.configure_scion_link(core1, core2, ScionLinkType::Core)?;

// Parent-child (provider-customer)
net.configure_scion_link(core, child, ScionLinkType::ParentChild)?;

// Peering (shortcuts)
net.configure_scion_link(as1, as2, ScionLinkType::Peering)?;
```

### 3. Run Beaconing

```rust
// Core beaconing (between core ASes)
net.scion_core_beaconing(1000)?;

// Intra-ISD beaconing (down hierarchy)
// Parameters: (timestamp, max_pcbs_per_interface)
net.scion_intra_isd_beaconing(1000, 50)?;
```

### 4. Register Segments

```rust
// Register up/down/core segments
// Parameter: max_segments_to_register
let (up_count, down_count, core_count) = net.scion_registration_round(50)?;
println!("Registered: {} up, {} down, {} core", up_count, down_count, core_count);
```

### 5. Lookup Paths

```rust
// Get ALL available paths (endhost selection model)
let all_paths = net.scion_lookup_paths(src, dst)?;

// Get paths with selection policy
use bgpsim::scion::ShortestPathPolicy;
let best_paths = net.scion_lookup_paths_with_selection(
    src, dst,
    &ShortestPathPolicy,
    3  // Want 3 best paths
)?;
```

## Advanced Topics

### Endhost Path Selection

SCION's key principle: **Endhosts select paths, not the network**.

```rust
// Lookup returns ALL available paths
let all_paths = net.scion_lookup_paths(src, dst)?;

// Application logic chooses based on requirements
let selected = all_paths.iter()
    .filter(|p| p.mtu >= 1500)           // Minimum MTU
    .filter(|p| p.as_path.len() <= 5)    // Maximum hops
    .min_by_key(|p| p.as_path.len())     // Prefer shortest
    .unwrap();
```

### Path Selection Policies

bgpsim provides several built-in policies:

```rust
use bgpsim::scion::{
    ShortestPathPolicy,
    HighestMtuPolicy,
    FirstNPolicy,
    AllPathsPolicy,
};

// Shortest path (fewest AS hops)
let paths = net.scion_lookup_paths_with_selection(
    src, dst, &ShortestPathPolicy, 5
)?;

// Highest MTU (best bandwidth)
let paths = net.scion_lookup_paths_with_selection(
    src, dst, &HighestMtuPolicy, 5
)?;

// First N paths (discovery order)
let paths = net.scion_lookup_paths_with_selection(
    src, dst, &FirstNPolicy, 10
)?;
```

### Peering Shortcuts

Peering links create shortcuts between paths:

```rust
// Setup: AS1 and AS2 have peering link
net.configure_scion_link(as1, as2, ScionLinkType::Peering)?;

// After beaconing, paths will include shortcut options
let paths = net.scion_lookup_paths(leaf1, leaf2)?;

// Some paths will use the peering shortcut
for path in paths {
    if path.peering_shortcut.is_some() {
        println!("Path uses peering shortcut!");
    }
}
```

### Inter-ISD Routing

Multi-hop paths across ISDs:

```rust
// Create multiple ISDs
let core1 = net.add_router("Core1", 65500);
let core2 = net.add_router("Core2", 65501);
let core3 = net.add_router("Core3", 65502);

net.enable_scion(core1, IsdAs::new(1, 110), true)?;
net.enable_scion(core2, IsdAs::new(2, 210), true)?;
net.enable_scion(core3, IsdAs::new(3, 310), true)?;

// Connect ISDs
net.add_link(core1, core2)?;
net.add_link(core2, core3)?;
net.configure_scion_link(core1, core2, ScionLinkType::Core)?;
net.configure_scion_link(core2, core3, ScionLinkType::Core)?;

// Run beaconing
net.scion_core_beaconing(1000)?;
net.scion_registration_round(50)?;

// Lookup paths across ISDs (ISD1 → ISD2 → ISD3)
let paths = net.scion_lookup_paths(core1, core3)?;
assert!(!paths.is_empty());
```

### Configurable Path Limits

For performance in large topologies:

```rust
// Default: up to 100 core path chains
let paths = net.scion_lookup_inter_isd_paths(src, dst)?;

// Custom limit: explore more paths (slower but more complete)
let more_paths = net.scion_lookup_inter_isd_paths_limited(src, dst, 500)?;

// Unlimited: exhaustive search (may be very slow)
let all_paths = net.scion_lookup_inter_isd_paths_limited(src, dst, 0)?;

// Fewer paths: faster exploration
let fewer_paths = net.scion_lookup_inter_isd_paths_limited(src, dst, 10)?;
```

### Failure Handling

```rust
// Simulate link failure
net.scion_handle_link_failure((router1, router2), current_time)?;

// Cleanup expired segments
let (expired_pcbs, expired_segs) = net.scion_cleanup_expired(current_time)?;
println!("Cleaned up {} PCBs, {} segments", expired_pcbs, expired_segs);

// Re-run beaconing to discover new paths
net.scion_core_beaconing(current_time)?;
net.scion_intra_isd_beaconing(current_time, 50)?;
net.scion_registration_round(50)?;

// Lookup paths again (should find alternative routes)
let new_paths = net.scion_lookup_paths(src, dst)?;
```

## Examples

### Example 1: Simple Intra-ISD Network

```rust
use bgpsim::prelude::*;

fn example_intra_isd() -> Result<(), NetworkError> {
    let mut net = Network::default();

    // Create 1 core + 3 leaf ASes
    let core = net.add_router("Core", 65500);
    let leaf1 = net.add_router("Leaf1", 65501);
    let leaf2 = net.add_router("Leaf2", 65502);
    let leaf3 = net.add_router("Leaf3", 65503);

    // Connect all leaves to core
    net.add_link(core, leaf1)?;
    net.add_link(core, leaf2)?;
    net.add_link(core, leaf3)?;

    // Enable SCION (all in ISD 1)
    net.enable_scion(core, IsdAs::new(1, 110), true)?;
    net.enable_scion(leaf1, IsdAs::new(1, 111), false)?;
    net.enable_scion(leaf2, IsdAs::new(1, 112), false)?;
    net.enable_scion(leaf3, IsdAs::new(1, 113), false)?;

    // Configure links
    net.configure_scion_link(core, leaf1, ScionLinkType::ParentChild)?;
    net.configure_scion_link(core, leaf2, ScionLinkType::ParentChild)?;
    net.configure_scion_link(core, leaf3, ScionLinkType::ParentChild)?;

    // Run protocols
    net.scion_core_beaconing(1000)?;
    net.scion_intra_isd_beaconing(1000, 50)?;
    net.scion_registration_round(50)?;

    // Lookup paths between leaves
    let paths = net.scion_lookup_paths(leaf1, leaf2)?;
    println!("Found {} paths", paths.len());

    Ok(())
}
```

### Example 2: Multi-ISD Network

```rust
fn example_multi_isd() -> Result<(), NetworkError> {
    let mut net = Network::default();

    // Create core ASes in different ISDs
    let core1 = net.add_router("Core1", 65500);
    let core2 = net.add_router("Core2", 65501);

    net.enable_scion(core1, IsdAs::new(1, 110), true)?;
    net.enable_scion(core2, IsdAs::new(2, 210), true)?;

    // Connect ISDs
    net.add_link(core1, core2)?;
    net.configure_scion_link(core1, core2, ScionLinkType::Core)?;

    // Add leaves in each ISD
    let leaf1 = net.add_router("Leaf1", 65502);
    let leaf2 = net.add_router("Leaf2", 65503);

    net.add_link(core1, leaf1)?;
    net.add_link(core2, leaf2)?;

    net.enable_scion(leaf1, IsdAs::new(1, 111), false)?;
    net.enable_scion(leaf2, IsdAs::new(2, 211), false)?;

    net.configure_scion_link(core1, leaf1, ScionLinkType::ParentChild)?;
    net.configure_scion_link(core2, leaf2, ScionLinkType::ParentChild)?;

    // Run protocols
    net.scion_core_beaconing(1000)?;
    net.scion_intra_isd_beaconing(1000, 50)?;
    net.scion_registration_round(50)?;

    // Lookup inter-ISD paths
    let paths = net.scion_lookup_paths(leaf1, leaf2)?;
    println!("Found {} inter-ISD paths", paths.len());

    Ok(())
}
```

### Example 3: Path Selection

```rust
use bgpsim::scion::{ShortestPathPolicy, HighestMtuPolicy};

fn example_path_selection() -> Result<(), NetworkError> {
    let mut net = setup_network()?; // Assume network setup

    let src = net.get_router_id("Leaf1")?;
    let dst = net.get_router_id("Leaf2")?;

    // Get all paths
    let all_paths = net.scion_lookup_paths(src, dst)?;
    println!("Total paths available: {}", all_paths.len());

    // Select shortest paths
    let shortest = net.scion_lookup_paths_with_selection(
        src, dst, &ShortestPathPolicy, 3
    )?;
    println!("3 shortest paths selected");

    // Select highest MTU paths
    let high_mtu = net.scion_lookup_paths_with_selection(
        src, dst, &HighestMtuPolicy, 3
    )?;
    println!("3 highest MTU paths selected");

    // Custom selection
    let custom = all_paths.iter()
        .filter(|p| p.as_path.len() <= 5)  // Max 5 hops
        .filter(|p| p.mtu >= 1400)         // Min 1400 MTU
        .take(5)
        .collect::<Vec<_>>();
    println!("5 custom-filtered paths");

    Ok(())
}
```

## API Reference

### Network Methods

```rust
// SCION Configuration
fn enable_scion(&mut self, router: RouterId, isd_as: IsdAs, is_core: bool) -> Result<()>
fn configure_scion_link(&mut self, a: RouterId, b: RouterId, link_type: ScionLinkType) -> Result<()>

// Beaconing
fn scion_core_beaconing(&mut self, timestamp: u32) -> Result<usize>
fn scion_intra_isd_beaconing(&mut self, timestamp: u32, max_select: usize) -> Result<usize>

// Registration
fn scion_registration_round(&mut self, max_select: usize) -> Result<(usize, usize, usize)>
fn scion_register_up_segments(&mut self, max_select: usize) -> Result<usize>
fn scion_register_down_segments(&mut self) -> Result<usize>
fn scion_register_core_segments(&mut self, max_select: usize) -> Result<usize>

// Path Lookup
fn scion_lookup_paths(&self, src: RouterId, dst: RouterId) -> Result<Vec<ForwardingPath<P>>>
fn scion_lookup_intra_isd_paths(&self, src: RouterId, dst: RouterId) -> Result<Vec<ForwardingPath<P>>>
fn scion_lookup_inter_isd_paths(&self, src: RouterId, dst: RouterId) -> Result<Vec<ForwardingPath<P>>>
fn scion_lookup_inter_isd_paths_limited(&self, src: RouterId, dst: RouterId, max_core_chains: usize) -> Result<Vec<ForwardingPath<P>>>

// Path Selection
fn scion_lookup_paths_with_selection(&self, src: RouterId, dst: RouterId, policy: &dyn PathSelectionPolicy<P>, max_count: usize) -> Result<Vec<ForwardingPath<P>>>

// Maintenance
fn scion_cleanup_expired(&mut self, current_time: u32) -> Result<(usize, usize)>
fn scion_handle_link_failure(&mut self, failed_link: (RouterId, RouterId), current_time: u32) -> Result<usize>
```

### Types

```rust
// ISD-AS identifier
struct IsdAs {
    isd: IsdNumber,  // 16-bit ISD
    asn: ScionAsn,   // 48-bit AS number
}

// Link types
enum ScionLinkType {
    Core,           // Between core ASes
    ParentChild,    // Provider-customer
    Peering,        // Lateral peering
}

// Forwarding path
struct ForwardingPath<P> {
    up_segment: Option<PathSegment<P>>,
    core_segment: Option<PathSegment<P>>,
    down_segment: Option<PathSegment<P>>,
    peering_shortcut: Option<PeeringShortcut>,
    as_path: Vec<IsdAs>,
    mtu: u16,
}
```

## Performance Tuning

### Default Parameters (Spec-Recommended)

bgpsim-scion uses defaults based on the official SCION specification
(**draft-dekater-scion-controlplane-10**):

```rust
// Available as constants in bgpsim
use bgpsim::DEFAULT_MAX_PCBS;           // 50
use bgpsim::DEFAULT_MAX_CORE_PCBS;      // 5
use bgpsim::DEFAULT_INTRA_ISD_INTERVAL; // 5 seconds
use bgpsim::DEFAULT_CORE_INTERVAL;      // 60 seconds
use bgpsim::DEFAULT_HOP_EXPIRATION;     // 21600 seconds (6 hours)
```

| Parameter | Default | Rationale |
|-----------|---------|-----------|
| `max_pcbs` | **50** | "At most 50 PCBs per child link" (spec line 1558) |
| `max_core_pcbs` | **5** | "at most 5 path segments to every destination AS" |
| `propagation_interval` | **5s (intra), 60s (core)** | Spec minimum values |
| `hop_expiration` | **6 hours** | "SHOULD be around 6 hours" (spec) |

### Beaconing Parameters

```rust
// Spec-recommended default (good path diversity + manageable overhead)
net.scion_intra_isd_beaconing(timestamp, 50)?;  // Propagate 50 best (DEFAULT)

// For testing/CI (faster, less diversity)
net.scion_intra_isd_beaconing(timestamp, 10)?;  // Propagate 10 best

// For maximum diversity research (large topologies)
net.scion_intra_isd_beaconing(timestamp, 100)?; // Propagate 100 best
```

**Performance Impact** (per spec analysis):
- AS with 100 parent links: 5,000 PCBs/interval @ max_pcbs=50
- Bandwidth: ~2.5 MB/s @ 5s intervals
- Processing: 10,000 signature verifications/second
- **Result**: "manageable with even modest consumer hardware"

### Registration Parameters

```rust
// Spec-recommended default
net.scion_registration_round(50)?;  // Register 50 best (DEFAULT)

// For faster testing
net.scion_registration_round(10)?;  // Register 10 best

// For maximum diversity
net.scion_registration_round(100)?; // Register 100 best
```

### Path Lookup Limits

```rust
// Default (100 core chains): good balance
let paths = net.scion_lookup_inter_isd_paths(src, dst)?;

// More exploration (slower, more paths)
let paths = net.scion_lookup_inter_isd_paths_limited(src, dst, 500)?;

// Less exploration (faster, fewer paths)
let paths = net.scion_lookup_inter_isd_paths_limited(src, dst, 10)?;
```

### Cleanup and Expiration

```rust
// Periodic cleanup to remove expired state
if timestamp % 300 == 0 {  // Every 300 time units
    net.scion_cleanup_expired(timestamp)?;
}
```

## Troubleshooting

### No Paths Found

**Problem**: `scion_lookup_paths()` returns empty vector

**Solutions**:
1. Check beaconing was run: `scion_core_beaconing()` and `scion_intra_isd_beaconing()`
2. Check registration was run: `scion_registration_round()`
3. Verify link types are correct
4. Check ISDs are properly configured
5. Ensure network is connected

```rust
// Debug: Check control service state
let cs = net.get_router(router_id)?.scion().ok_or("SCION not enabled")?;
println!("Beacon store: {} PCBs", cs.beacon_store.total_count());
println!("Path database: {} segments", cs.path_database.total_count());
```

### Performance Issues

**Problem**: Slow path lookup

**Solutions**:
1. Use path limits: `scion_lookup_inter_isd_paths_limited(src, dst, 50)`
2. Reduce beaconing frequency
3. Register fewer segments

### Unexpected Path Results

**Problem**: Not seeing expected paths

**Check**:
1. Link types: Core links only between core ASes
2. Parent-child direction: From parent's perspective
3. Peering links: Both ASes must be non-core (usually)
4. Registration: Verify segments are registered at correct core AS

## Further Reading

- [SCION Specification](https://docs.scion.org/)
- [SCION Architecture Book](https://scion-architecture.net/)
- [Performance Documentation](./PHASE12_PERFORMANCE.md)
- [Endhost Path Selection](./ENDHOST_PATH_SELECTION.md)
- [Implementation Tracking](./IMPLEMENTATION_TRACKING.md)


