# SCION Research Use Cases in bgpsim

**Date**: 2025-10-28
**Purpose**: Guide for researchers using bgpsim-scion

## Overview

This document outlines research use cases enabled by bgpsim's SCION implementation. Each use case includes research questions, methodology, and code templates.

## Table of Contents

1. [Path Diversity Analysis](#1-path-diversity-analysis)
2. [Failure Recovery Performance](#2-failure-recovery-performance)
3. [Path Selection Policy Evaluation](#3-path-selection-policy-evaluation)
4. [ISD Design and Placement](#4-isd-design-and-placement)
5. [Peering Economics](#5-peering-economics)
6. [Protocol Comparison](#6-protocol-comparison)
7. [Transition and Migration](#7-transition-and-migration)
8. [Security and Resilience](#8-security-and-resilience)

## 1. Path Diversity Analysis

### Research Question
**How does network topology affect path diversity in SCION vs BGP?**

### Methodology
1. Generate various network topologies (mesh, tree, random)
2. Enable SCION on all routers
3. Measure path count per source-destination pair
4. Compare with BGP (always 1 path)

### Code Template
```rust
use bgpsim::prelude::*;

fn analyze_path_diversity() -> Result<()> {
    let mut results = Vec::new();

    for size in [10, 20, 50, 100] {
        // Generate topology
        let mut net = generate_random_topology(size)?;

        // Enable SCION
        for router in net.get_routers() {
            let isd = (router.id().index() / 20) as u16 + 1;  // Group into ISDs
            let asn = 100 + router.id().index() as u64;
            let is_core = router.id().index() % 10 == 0;  // Every 10th AS is core

            net.enable_scion(router.id(), IsdAs::new(isd, asn), is_core)?;
        }

        // Configure links
        for (r1, r2) in net.get_topology().edges() {
            net.configure_scion_link(r1, r2, determine_link_type(r1, r2))?;
        }

        // Run SCION protocols
        net.scion_core_beaconing(1000)?;
        net.scion_intra_isd_beaconing(1000, 5)?;
        net.scion_registration_round(5)?;

        // Measure path diversity
        let mut path_counts = Vec::new();
        for src in net.get_routers() {
            for dst in net.get_routers() {
                if src.id() != dst.id() {
                    let paths = net.scion_lookup_paths(src.id(), dst.id())?;
                    path_counts.push(paths.len());
                }
            }
        }

        let avg_paths = path_counts.iter().sum::<usize>() / path_counts.len();
        let max_paths = path_counts.iter().max().unwrap();

        results.push((size, avg_paths, *max_paths));
    }

    // Output results
    println!("Size,AvgPaths,MaxPaths");
    for (size, avg, max) in results {
        println!("{},{},{}", size, avg, max);
    }

    Ok(())
}
```

### Expected Insights
- Path diversity increases with network density
- Hierarchical topologies provide fewer but more stable paths
- Peering links significantly increase path count

## 2. Failure Recovery Performance

### Research Question
**How quickly can SCION recover from link failures compared to BGP?**

### Methodology
1. Create network with multiple paths
2. Establish baseline connectivity
3. Simulate link failure
4. Measure time until connectivity restored
5. Compare SCION (instant failover) vs BGP (reconvergence)

### Code Template
```rust
fn measure_failure_recovery() -> Result<()> {
    let mut net = create_multi_path_network()?;

    // Setup both protocols
    setup_bgp(&mut net)?;
    setup_scion(&mut net)?;

    let critical_link = (core1, core2);

    // === BGP Measurement ===
    let bgp_start = Instant::now();

    // Fail link
    net.set_link_weight(critical_link.0, critical_link.1, f64::INFINITY)?;

    // Wait for BGP convergence
    net.manual_simulation()?;

    let bgp_recovery_time = bgp_start.elapsed();

    // === SCION Measurement ===

    // Reset network
    net.set_link_weight(critical_link.0, critical_link.1, 1.0)?;

    // Get pre-failure paths
    let before_paths = net.scion_lookup_paths(leaf1, leaf2)?;

    let scion_start = Instant::now();

    // Fail link
    net.scion_handle_link_failure(critical_link, 1000)?;

    // Get remaining paths (instant)
    let after_paths = net.scion_lookup_paths(leaf1, leaf2)?
        .into_iter()
        .filter(|p| !p.uses_link(critical_link.0, critical_link.1))
        .collect::<Vec<_>>();

    let scion_failover_time = scion_start.elapsed();

    // Results
    println!("BGP reconvergence time: {:?}", bgp_recovery_time);
    println!("SCION failover time: {:?}", scion_failover_time);
    println!("SCION had {} alternate paths available", after_paths.len());

    Ok(())
}
```

### Expected Insights
- SCION failover is orders of magnitude faster (microseconds vs seconds)
- SCION provides graceful degradation (remaining paths still usable)
- BGP requires full network reconvergence

## 3. Path Selection Policy Evaluation

### Research Question
**How do different path selection policies affect application performance?**

### Methodology
1. Create network with diverse paths (vary MTU, length, etc.)
2. Implement custom selection policies
3. Measure resulting path properties
4. Compare against application requirements

### Code Template
```rust
use bgpsim::scion::{PathSelectionPolicy, ForwardingPath};

struct LatencySensitivePolicy;
impl<P: Prefix> PathSelectionPolicy<P> for LatencySensitivePolicy {
    fn select_paths(&self, paths: &[&ForwardingPath<P>], max: usize) -> Vec<usize> {
        let mut indexed: Vec<_> = paths.iter().enumerate().collect();

        // Sort by: 1) AS path length (latency proxy), 2) MTU
        indexed.sort_by_key(|(_, p)| (p.as_path.len(), -(p.mtu as i32)));

        indexed.into_iter().take(max).map(|(i, _)| i).collect()
    }
}

struct BandwidthSensitivePolicy;
impl<P: Prefix> PathSelectionPolicy<P> for BandwidthSensitivePolicy {
    fn select_paths(&self, paths: &[&ForwardingPath<P>], max: usize) -> Vec<usize> {
        let mut indexed: Vec<_> = paths.iter().enumerate().collect();

        // Sort by: 1) MTU (bandwidth proxy), 2) path length
        indexed.sort_by_key(|(_, p)| (-(p.mtu as i32), p.as_path.len()));

        indexed.into_iter().take(max).map(|(i, _)| i).collect()
    }
}

fn evaluate_policies() -> Result<()> {
    let mut net = create_diverse_network()?;

    let src = net.get_router_id("Source")?;
    let dst = net.get_router_id("Dest")?;

    // Get all available paths
    let all_paths = net.scion_lookup_paths(src, dst)?;

    // Test policies
    let policies: Vec<(&str, Box<dyn PathSelectionPolicy<SimplePrefix>>)> = vec![
        ("Shortest", Box::new(ShortestPathPolicy)),
        ("Highest MTU", Box::new(HighestMtuPolicy)),
        ("Latency-sensitive", Box::new(LatencySensitivePolicy)),
        ("Bandwidth-sensitive", Box::new(BandwidthSensitivePolicy)),
    ];

    for (name, policy) in policies {
        let selected = net.scion_lookup_paths_with_selection(
            src, dst, policy.as_ref(), 3
        )?;

        println!("\n{} Policy:", name);
        for (i, path) in selected.iter().enumerate() {
            println!("  Path {}: hops={}, MTU={}, ASes={:?}",
                     i + 1, path.as_path.len(), path.mtu, path.as_path);
        }
    }

    Ok(())
}
```

### Expected Insights
- Different policies optimize for different metrics
- Trade-offs between latency, bandwidth, and resilience
- No single "best" policy - depends on application needs

## 4. ISD Design and Placement

### Research Question
**How should ISDs be designed to optimize routing performance?**

### Methodology
1. Generate large network topology
2. Try different ISD boundary placements
3. Measure: convergence time, path lengths, core overhead
4. Identify optimal ISD sizes and topologies

### Code Template
```rust
fn evaluate_isd_designs() -> Result<()> {
    let topology = generate_large_network(100)?;  // 100 ASes

    // Try different ISD designs
    let designs = vec![
        ("Single ISD", vec![100]),              // All in one ISD
        ("Geographic (5)", vec![20, 20, 20, 20, 20]),  // 5 ISDs of 20
        ("Hierarchical", vec![10, 30, 30, 30]),   // One large, three small
        ("Many small", vec![10; 10]),            // 10 ISDs of 10
    ];

    for (name, isd_sizes) in designs {
        let mut net = topology.clone();

        // Assign ISDs
        assign_isds(&mut net, &isd_sizes)?;

        // Run protocols
        let start = Instant::now();
        net.scion_core_beaconing(1000)?;
        net.scion_intra_isd_beaconing(1000, 5)?;
        net.scion_registration_round(5)?;
        let setup_time = start.elapsed();

        // Measure path properties
        let (avg_paths, avg_length) = measure_paths(&net)?;

        // Measure core load
        let core_segments = count_core_segments(&net)?;

        println!("{}: setup={:?}, avg_paths={:.1}, avg_len={:.1}, core_segs={}",
                 name, setup_time, avg_paths, avg_length, core_segments);
    }

    Ok(())
}
```

### Expected Insights
- Larger ISDs reduce core overhead but increase intra-ISD complexity
- Geographic boundaries align well with trust domains
- Too many small ISDs increases core beaconing overhead

## 5. Peering Economics

### Research Question
**What is the value of peering links for path diversity?**

### Methodology
1. Create network without peering
2. Measure baseline path diversity
3. Add peering links between ASes
4. Measure improvement in path count and quality

### Code Template
```rust
fn analyze_peering_value() -> Result<()> {
    let mut net = create_hierarchical_network()?;

    // Baseline: no peering
    net.scion_core_beaconing(1000)?;
    net.scion_intra_isd_beaconing(1000, 5)?;
    net.scion_registration_round(5)?;

    let baseline_paths = measure_all_paths(&net)?;

    // Add peering links
    let peering_candidates = identify_peering_candidates(&net)?;

    for (as1, as2) in peering_candidates {
        net.add_link(as1, as2)?;
        net.configure_scion_link(as1, as2, ScionLinkType::Peering)?;
    }

    // Re-run protocols
    net.scion_core_beaconing(2000)?;
    net.scion_intra_isd_beaconing(2000, 5)?;
    net.scion_registration_round(5)?;

    let peering_paths = measure_all_paths(&net)?;

    // Analyze improvement
    let improvement = (peering_paths as f64 / baseline_paths as f64 - 1.0) * 100.0;

    println!("Baseline paths: {}", baseline_paths);
    println!("With peering: {}", peering_paths);
    println!("Improvement: {:.1}%", improvement);

    Ok(())
}
```

### Expected Insights
- Peering links provide significant shortcut opportunities
- Strategic peering placement maximizes benefit
- Diminishing returns with too many peering links

## 6. Protocol Comparison

### Research Question
**How does SCION compare to BGP in terms of convergence, resilience, and overhead?**

### Methodology
1. Run same topology with both BGP and SCION
2. Measure: convergence time, message count, path diversity
3. Simulate failures and measure recovery
4. Compare control plane overhead

### Code Template
```rust
fn compare_protocols() -> Result<()> {
    let topology = create_standard_topology()?;

    // === BGP Experiment ===
    let mut bgp_net = topology.clone();
    setup_bgp(&mut bgp_net)?;

    let bgp_start = Instant::now();
    bgp_net.manual_simulation()?;
    let bgp_convergence = bgp_start.elapsed();

    let bgp_messages = count_bgp_updates(&bgp_net)?;
    let bgp_paths_per_dst = 1;  // BGP always has 1 path

    // === SCION Experiment ===
    let mut scion_net = topology.clone();
    setup_scion(&mut scion_net)?;

    let scion_start = Instant::now();
    scion_net.scion_core_beaconing(1000)?;
    scion_net.scion_intra_isd_beaconing(1000, 5)?;
    scion_net.scion_registration_round(5)?;
    let scion_setup = scion_start.elapsed();

    let scion_pcbs = count_total_pcbs(&scion_net)?;
    let scion_avg_paths = measure_avg_path_diversity(&scion_net)?;

    // Results
    println!("=== Protocol Comparison ===");
    println!("BGP convergence: {:?}", bgp_convergence);
    println!("SCION setup: {:?}", scion_setup);
    println!("BGP UPDATE messages: {}", bgp_messages);
    println!("SCION PCBs propagated: {}", scion_pcbs);
    println!("BGP paths per destination: {}", bgp_paths_per_dst);
    println!("SCION avg paths per destination: {:.1}", scion_avg_paths);

    Ok(())
}
```

### Expected Insights
- SCION setup is deterministic and predictable
- SCION provides significantly more path diversity
- BGP has lower overhead for small networks
- SCION scales better for large, stable networks

## 7. Transition and Migration

### Research Question
**How can networks transition from BGP to SCION?**

### Methodology
1. Create hybrid network (some BGP, some SCION)
2. Model gateway ASes supporting both protocols
3. Measure connectivity and path quality during transition
4. Identify optimal migration strategies

### Code Template
```rust
fn study_transition() -> Result<()> {
    let mut net = Network::default();

    // Phase 1: All BGP (legacy)
    let legacy_ases = create_bgp_network(&mut net, 50)?;

    // Phase 2: Add SCION-enabled ASes
    let scion_ases = create_scion_island(&mut net, 20)?;

    // Phase 3: Gateway ASes (support both)
    let gateways = create_gateways(&mut net, 5)?;

    // Connect gateways to both networks
    for gw in &gateways {
        // BGP peering
        for legacy in &legacy_ases[..5] {
            net.set_bgp_session(*gw, *legacy, Some(BgpSessionType::EBgp))?;
        }

        // SCION links
        for scion in &scion_ases[..5] {
            net.add_link(*gw, *scion)?;
            net.configure_scion_link(*gw, *scion, ScionLinkType::ParentChild)?;
        }
    }

    // Measure connectivity
    let bgp_to_bgp = test_connectivity(&net, &legacy_ases, &legacy_ases)?;
    let scion_to_scion = test_scion_connectivity(&net, &scion_ases, &scion_ases)?;
    let cross_protocol = test_cross_connectivity(&net, &legacy_ases, &scion_ases, &gateways)?;

    println!("BGP-to-BGP: {}%", bgp_to_bgp * 100.0);
    println!("SCION-to-SCION: {}%", scion_to_scion * 100.0);
    println!("Cross-protocol: {}%", cross_protocol * 100.0);

    Ok(())
}
```

### Expected Insights
- Gradual migration is feasible
- Gateway ASes are critical for transition
- Hybrid mode provides path diversity benefits

## 8. Security and Resilience

### Research Question
**How resilient is SCION to path pollution and attacks?**

### Methodology
1. Create network with malicious AS
2. Model attack scenarios (PCB pollution, path hijacking)
3. Measure impact on path availability
4. Compare with BGP prefix hijacking

### Code Template
```rust
fn study_resilience() -> Result<()> {
    let mut net = create_multi_isd_network()?;

    let malicious_as = identify_strategic_as(&net)?;

    // Baseline
    let baseline_paths = measure_all_paths(&net)?;

    // Attack Scenario 1: PCB Flooding
    // (In simulation: malicious AS creates excessive PCBs)
    pollute_beacons(&mut net, malicious_as, 1000)?;  // Flood with 1000 PCBs

    net.scion_core_beaconing(2000)?;
    net.scion_intra_isd_beaconing(2000, 5)?;
    net.scion_registration_round(5)?;

    let flooded_paths = measure_all_paths(&net)?;

    // ISD isolation should limit damage
    let affected_isd = get_isd(malicious_as);
    let other_isds_affected = measure_cross_isd_impact(&net, affected_isd)?;

    println!("Baseline paths: {}", baseline_paths);
    println!("After flooding: {}", flooded_paths);
    println!("Other ISDs affected: {:.1}%", other_isds_affected * 100.0);

    Ok(())
}
```

### Expected Insights
- ISD isolation limits attack impact
- PCB selection policies filter malicious beacons
- Valley-free property prevents many attacks
- Endhost can avoid suspicious paths

## Running Research Experiments

### Batch Experiments
```bash
# Run all experiments
cargo run --release --bin research_experiments

# Run specific use case
cargo run --release --bin research_experiments -- --use-case path-diversity

# Output to CSV
cargo run --release --bin research_experiments -- --output results.csv
```

### Reproducibility
- Use fixed random seeds for topology generation
- Document all parameters in results
- Save network topologies for replay
- Version control experiment code

## Further Research Directions

1. **Multi-path Transport**: Model applications using multiple paths simultaneously
2. **Dynamic Traffic Engineering**: Adapt path selection based on load
3. **Colibri Integration**: Add bandwidth reservation layer
4. **Hidden Paths**: Model access-controlled path segments
5. **Machine Learning**: Train policies based on network conditions
6. **Economic Models**: Cost-benefit analysis of SCION deployment

## Conclusion

bgpsim-scion enables a wide range of research questions about next-generation routing. The deterministic simulation environment allows for controlled experiments and reproducible results.

For more information:
- User Guide: `SCION_USER_GUIDE.md`
- Comparison: `SCION_VS_BGP_COMPARISON.md`
- Performance: `PHASE12_PERFORMANCE.md`
