# SCION vs BGP: A Comparison in bgpsim

**Date**: 2025-10-28
**Purpose**: Guide for researchers comparing SCION and BGP

## Table of Contents

1. [Overview](#overview)
2. [Architectural Differences](#architectural-differences)
3. [Side-by-Side Comparison](#side-by-side-comparison)
4. [Code Examples](#code-examples)
5. [Research Scenarios](#research-scenarios)
6. [Performance Comparison](#performance-comparison)

## Overview

This document compares SCION and BGP implementations in bgpsim, providing insights for researchers studying next-generation routing architectures.

### When to Use Which?

**Use BGP** when studying:
- Current Internet routing
- Single best path selection
- Convergence dynamics
- Policy-based routing

**Use SCION** when studying:
- Path diversity and multipath
- Endhost path control
- Fault isolation (ISDs)
- Path-aware applications

**Use Both** when studying:
- Transition scenarios
- Hybrid networks
- Comparative analysis
- Migration strategies

## Architectural Differences

### Fundamental Philosophy

| Aspect | BGP | SCION |
|--------|-----|-------|
| **Path Selection** | Network decides (routers) | Endhost decides (applications) |
| **Available Paths** | One best path | Multiple paths available |
| **Path Information** | Hidden from endhosts | Visible to endhosts |
| **Topology** | Flat AS graph | Hierarchical ISDs + ASes |
| **Trust** | Transitive trust | Isolated trust domains |
| **Failures** | Network reconvergence | Instant endhost failover |

### Routing Model

**BGP**: Distributed path-vector protocol
- Each router computes best path
- Best path announced to neighbors
- Convergence through message propagation
- Policy applied locally at routers

**SCION**: Beaconing + Path lookup
- Core ASes create PCBs (beacons)
- PCBs propagate down hierarchy
- ASes register path segments
- Endhosts query and combine segments

### Control Plane vs Data Plane

**BGP**:
```
Control: Routers exchange UPDATE messages
Data: Packets forwarded based on FIB
Decision: Routers choose paths
```

**SCION**:
```
Control: Periodic beaconing + registration
Data: Packets carry path in header
Decision: Endhosts choose paths
```

## Side-by-Side Comparison

### Network Setup

**BGP**:
```rust
use bgpsim::prelude::*;

let mut net = Network::default();

// Add routers
let r1 = net.add_router("R1", 65001);
let r2 = net.add_router("R2", 65002);

// Add BGP session
net.set_bgp_session(r1, r2, Some(BgpSessionType::EBgp))?;

// Advertise routes
let prefix = Prefix::from(0);
net.advertise_external_route(r1, prefix, vec![65001], None, None)?;
```

**SCION**:
```rust
use bgpsim::prelude::*;
use bgpsim::scion::{IsdAs, ScionLinkType};

let mut net = Network::default();

// Add routers
let r1 = net.add_router("R1", 65001);
let r2 = net.add_router("R2", 65002);

// Enable SCION
net.enable_scion(r1, IsdAs::new(1, 110), true)?;
net.enable_scion(r2, IsdAs::new(1, 111), false)?;

// Configure link
net.add_link(r1, r2)?;
net.configure_scion_link(r1, r2, ScionLinkType::ParentChild)?;

// Run beaconing
net.scion_core_beaconing(1000)?;
net.scion_intra_isd_beaconing(1000, 5)?;
net.scion_registration_round(5)?;
```

### Path Discovery

**BGP**:
```rust
// Path discovery is automatic through UPDATE messages
// Routers learn paths through neighbor advertisements

// Query routing table
let route = net.get_route(router, prefix)?;
if let Some(next_hop) = route {
    println!("Next hop: {:?}", next_hop);
}
```

**SCION**:
```rust
// Explicit path lookup by endhosts
let paths = net.scion_lookup_paths(src, dst)?;

println!("Available paths: {}", paths.len());
for (i, path) in paths.iter().enumerate() {
    println!("Path {}: {:?}", i, path.as_path);
}
```

### Path Selection

**BGP**:
```rust
// Router selects ONE best path using decision process
// Order: LOCAL_PREF > AS_PATH_LENGTH > ORIGIN > MED > ...

// Application has NO choice
// Network decides the path
```

**SCION**:
```rust
// Endhost/application selects from ALL available paths

// Get all paths
let all_paths = net.scion_lookup_paths(src, dst)?;

// Application chooses based on requirements
let my_path = all_paths.iter()
    .filter(|p| p.mtu >= 1500)       // Bandwidth requirement
    .filter(|p| p.as_path.len() <= 4) // Latency requirement
    .next()
    .unwrap();

// Or use selection policy
let paths = net.scion_lookup_paths_with_selection(
    src, dst,
    &ShortestPathPolicy,  // Policy choice
    5  // Want 5 options
)?;
```

### Failure Handling

**BGP**:
```rust
// 1. Link fails
net.set_link_weight(r1, r2, f64::INFINITY)?;

// 2. Network reconverges (takes time)
// - Withdrawal messages propagate
// - Routers recompute best paths
// - New UPDATEs sent

// 3. Applications experience outage until convergence
```

**SCION**:
```rust
// 1. Link fails
net.scion_handle_link_failure((r1, r2), current_time)?;

// 2. Endhosts switch to alternate path (instant)
let remaining_paths = net.scion_lookup_paths(src, dst)?
    .into_iter()
    .filter(|p| !p.uses_link(r1, r2))
    .collect();

// 3. Background: beaconing discovers new paths
net.scion_core_beaconing(current_time)?;
net.scion_intra_isd_beaconing(current_time, 5)?;

// 4. No application outage if alternate paths exist
```

## Code Examples

### Example 1: Simple Network Comparison

**BGP Version**:
```rust
fn bgp_simple_network() -> Result<()> {
    let mut net = Network::default();

    // Create 3-AS network: AS1 -- AS2 -- AS3
    let as1 = net.add_router("AS1", 65001);
    let as2 = net.add_router("AS2", 65002);
    let as3 = net.add_router("AS3", 65003);

    net.add_link(as1, as2)?;
    net.add_link(as2, as3)?;

    net.set_bgp_session(as1, as2, Some(BgpSessionType::EBgp))?;
    net.set_bgp_session(as2, as3, Some(BgpSessionType::EBgp))?;

    // AS1 advertises prefix
    let prefix = Prefix::from(0);
    net.advertise_external_route(as1, prefix, vec![65001], None, None)?;

    // BGP converges
    net.manual_simulation()?;

    // AS3 learns ONE path to prefix
    let route = net.get_route(as3, prefix)?;
    println!("AS3 route: {:?}", route);  // Single best path

    Ok(())
}
```

**SCION Version**:
```rust
fn scion_simple_network() -> Result<()> {
    let mut net = Network::default();

    // Create 3-AS network: AS1 (core) -- AS2 -- AS3
    let as1 = net.add_router("AS1", 65001);
    let as2 = net.add_router("AS2", 65002);
    let as3 = net.add_router("AS3", 65003);

    net.add_link(as1, as2)?;
    net.add_link(as2, as3)?;

    // AS1 is core, AS2 and AS3 are non-core
    net.enable_scion(as1, IsdAs::new(1, 110), true)?;
    net.enable_scion(as2, IsdAs::new(1, 111), false)?;
    net.enable_scion(as3, IsdAs::new(1, 112), false)?;

    net.configure_scion_link(as1, as2, ScionLinkType::ParentChild)?;
    net.configure_scion_link(as1, as3, ScionLinkType::ParentChild)?;

    // Run SCION protocols
    net.scion_core_beaconing(1000)?;
    net.scion_intra_isd_beaconing(1000, 5)?;
    net.scion_registration_round(5)?;

    // AS3 can query ALL paths to AS2
    let paths = net.scion_lookup_paths(as3, as2)?;
    println!("AS3 has {} paths to AS2", paths.len());  // Multiple paths

    Ok(())
}
```

### Example 2: Path Diversity

**BGP**:
```rust
fn bgp_limited_diversity() -> Result<()> {
    let mut net = create_diamond_topology()?;

    // Diamond: S -- A -- D
    //          |    X    |
    //          +-- B --+

    // BGP will choose ONE path based on decision process
    // Even though two paths exist (S-A-D and S-B-D)

    let src = net.get_router_id("S")?;
    let dst = net.get_router_id("D")?;

    // Only one active path
    let route = net.get_forwarding_path(src, dst)?;
    println!("BGP uses: {:?}", route);  // One path

    Ok(())
}
```

**SCION**:
```rust
fn scion_path_diversity() -> Result<()> {
    let mut net = create_diamond_topology_scion()?;

    // Same diamond topology
    // SCION provides BOTH paths to application

    let src = net.get_router_id("S")?;
    let dst = net.get_router_id("D")?;

    let paths = net.scion_lookup_paths(src, dst)?;
    println!("SCION provides {} paths", paths.len());  // Both paths

    // Application can choose based on requirements
    for (i, path) in paths.iter().enumerate() {
        println!("Path {}: {:?}, MTU: {}", i, path.as_path, path.mtu);
    }

    Ok(())
}
```

### Example 3: Failure Recovery

**BGP**:
```rust
fn bgp_failure_recovery() -> Result<()> {
    let mut net = setup_network()?;

    let link = (router1, router2);

    // Before failure
    let before = net.get_route(src, prefix)?;

    // Simulate failure
    net.set_link_weight(link.0, link.1, f64::INFINITY)?;

    // Trigger BGP convergence
    net.manual_simulation()?;

    // After convergence (takes time)
    let after = net.get_route(src, prefix)?;

    // Convergence time is non-zero
    println!("Convergence required message propagation");

    Ok(())
}
```

**SCION**:
```rust
fn scion_failure_recovery() -> Result<()> {
    let mut net = setup_network()?;

    // Before failure - get all paths
    let all_paths = net.scion_lookup_paths(src, dst)?;
    println!("{} paths available", all_paths.len());

    // Simulate failure
    net.scion_handle_link_failure((router1, router2), time)?;

    // Immediate failover - use different path
    let remaining = net.scion_lookup_paths(src, dst)?;
    println!("{} paths still available", remaining.len());

    // NO convergence delay for existing paths
    // Application switches instantly

    // Background: discover new paths
    net.scion_core_beaconing(time)?;
    net.scion_intra_isd_beaconing(time, 5)?;

    Ok(())
}
```

## Research Scenarios

### Scenario 1: Path Diversity Analysis

**Research Question**: How many paths are available in typical topologies?

```rust
fn compare_path_availability() -> Result<()> {
    let mut net = create_research_topology()?;

    // BGP: Count reachable destinations
    let bgp_reachable = net.get_routers()
        .filter(|r| net.get_route(src, r).is_some())
        .count();

    // SCION: Count paths per destination
    let scion_paths: Vec<_> = net.get_routers()
        .map(|dst| net.scion_lookup_paths(src, dst).unwrap().len())
        .collect();

    println!("BGP: {} reachable destinations (1 path each)", bgp_reachable);
    println!("SCION: {} avg paths per destination",
             scion_paths.iter().sum::<usize>() / scion_paths.len());

    Ok(())
}
```

### Scenario 2: Convergence Time

**Research Question**: How fast do protocols adapt to failures?

```rust
fn compare_convergence() -> Result<()> {
    // BGP: Measure until routing tables stable
    let bgp_start = Instant::now();
    net.manual_simulation()?;
    let bgp_time = bgp_start.elapsed();

    // SCION: Beaconing is periodic
    let scion_start = Instant::now();
    net.scion_core_beaconing(time)?;
    net.scion_intra_isd_beaconing(time, 5)?;
    net.scion_registration_round(5)?;
    let scion_time = scion_start.elapsed();

    // But SCION endhosts can failover instantly to cached paths
    println!("BGP convergence: {:?}", bgp_time);
    println!("SCION beaconing: {:?}", scion_time);
    println!("SCION failover: instant (uses cached paths)");

    Ok(())
}
```

### Scenario 3: Policy Compliance

**Research Question**: Can applications enforce their policies?

```rust
fn compare_policy_control() -> Result<()> {
    // BGP: Network operator controls routing
    // Application has NO say in path selection

    // SCION: Application controls path selection
    let all_paths = net.scion_lookup_paths(src, dst)?;

    // Application policy: avoid country X, prefer low latency
    let compliant_paths = all_paths.iter()
        .filter(|p| !p.traverses_country("X"))
        .filter(|p| p.as_path.len() <= 5)
        .collect::<Vec<_>>();

    println!("BGP: Application cannot enforce policy");
    println!("SCION: {} paths match application policy", compliant_paths.len());

    Ok(())
}
```

### Scenario 4: Transition Study

**Research Question**: Can SCION and BGP coexist?

```rust
fn hybrid_network() -> Result<()> {
    let mut net = Network::default();

    // Some ASes use BGP
    let bgp_as1 = net.add_router("BGP1", 65001);
    let bgp_as2 = net.add_router("BGP2", 65002);

    net.set_bgp_session(bgp_as1, bgp_as2, Some(BgpSessionType::EBgp))?;

    // Some ASes use SCION
    let scion_as1 = net.add_router("SCION1", 65003);
    let scion_as2 = net.add_router("SCION2", 65004);

    net.enable_scion(scion_as1, IsdAs::new(1, 110), true)?;
    net.enable_scion(scion_as2, IsdAs::new(1, 111), false)?;
    net.add_link(scion_as1, scion_as2)?;
    net.configure_scion_link(scion_as1, scion_as2, ScionLinkType::ParentChild)?;

    // Gateway AS supports both
    let gateway = net.add_router("Gateway", 65005);
    net.set_bgp_session(gateway, bgp_as2, Some(BgpSessionType::EBgp))?;
    net.enable_scion(gateway, IsdAs::new(1, 112), false)?;

    // Study: How does traffic flow between protocols?

    Ok(())
}
```

## Performance Comparison

### Control Plane Overhead

| Metric | BGP | SCION |
|--------|-----|-------|
| **Message Type** | UPDATE (event-driven) | Beacon (periodic) |
| **Frequency** | On topology change | Every 60s (configurable) |
| **Overhead** | O(N) per change | O(N) per interval |
| **Convergence** | Variable, depends on change | Fixed, based on interval |
| **State per Router** | One best path per prefix | Multiple segments |

### Data Plane

| Aspect | BGP | SCION |
|--------|-----|-------|
| **Header** | IP header only | IP + SCION path header |
| **Path Info** | Not in packet | Carried in packet |
| **Forwarding** | Hop-by-hop lookup | Source-specified path |
| **Per-packet** | FIB lookup | Path validation (MAC) |

### Scalability

**BGP**:
- State: O(N×P) where N=ASes, P=prefixes
- Convergence: O(N×D) where D=diameter
- Per-router: Best path per prefix

**SCION**:
- State: O(N×S) where N=ASes, S=segments
- Beaconing: O(N×I) where I=interval
- Per-AS: Multiple segments per destination

## Summary Table

| Feature | BGP | SCION |
|---------|-----|-------|
| **Paths Available** | 1 (best) | Many (all valid) |
| **Who Chooses** | Network | Endhost/Application |
| **Failure Recovery** | Reconvergence | Instant failover |
| **Path Visibility** | Hidden | Visible |
| **Trust Model** | Transitive | Isolated (ISDs) |
| **Hierarchy** | Flat | Hierarchical |
| **Control** | Distributed | Beaconing |
| **Policy** | Operator-controlled | Application-controlled |
| **Multipath** | No (single best) | Yes (native) |
| **Simulation Speed** | Event-driven | Periodic + lookup |

## When to Use What in Research

### Use BGP when studying:
- Current Internet behavior
- Routing policies (import/export)
- Prefix hijacking and security
- Economic relationships
- Single-path routing

### Use SCION when studying:
- Path diversity and resilience
- Application-layer routing
- ISD isolation and trust
- Multipath communication
- Endhost empowerment

### Use Both when studying:
- Protocol comparison
- Transition scenarios
- Hybrid architectures
- Performance trade-offs
- Deployment strategies

## Conclusion

BGP and SCION represent fundamentally different approaches to inter-domain routing:

**BGP**: Network-centric, single best path, operator control
**SCION**: Endhost-centric, multiple paths, application control

bgpsim enables researchers to:
1. Study both protocols in isolation
2. Compare them on the same topology
3. Explore hybrid scenarios
4. Measure performance trade-offs

Both implementations maintain high fidelity to their respective specifications while enabling fast, deterministic simulation.
