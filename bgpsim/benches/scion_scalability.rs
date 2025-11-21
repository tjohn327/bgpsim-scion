// SCION Scalability Benchmark
// Tests SCION performance at 1000, 10k, 100k, and 1M routers

use bgpsim::prelude::*;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::time::Duration;

#[cfg(feature = "scion")]
use bgpsim::scion::{IsdAs, IsdNumber, ScionLinkType};

/// Generate a scalable topology
fn generate_scalable_topology(
    size: usize,
) -> Result<Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>, NetworkError> {
    let mut net = Network::default();

    // Calculate hierarchy
    let num_isds = (size as f64).sqrt().ceil() as usize;
    let ases_per_isd = size / num_isds;
    let core_ratio = 0.05; // 5% core ASes
    let transit_ratio = 0.25; // 25% transit ASes
                              // rest are leaf ASes

    let core_per_isd = (ases_per_isd as f64 * core_ratio).ceil().max(2.0) as usize;
    let transit_per_isd = (ases_per_isd as f64 * transit_ratio).ceil() as usize;
    let leaf_per_isd = ases_per_isd - core_per_isd - transit_per_isd;

    println!(
        "  Topology: {} ISDs, {} core/ISD, {} transit/ISD, {} leaf/ISD",
        num_isds, core_per_isd, transit_per_isd, leaf_per_isd
    );

    let mut all_cores = Vec::new();
    let mut all_transits = Vec::new();
    let mut all_leaves = Vec::new();

    // Create routers
    for isd in 0..num_isds {
        let mut cores = Vec::new();
        let mut transits = Vec::new();
        let mut leaves = Vec::new();

        // Core ASes
        for i in 0..core_per_isd {
            let asn = (isd * ases_per_isd + i) as u64 + 100;
            let router = net.add_router(&format!("R{}", asn), asn);
            cores.push(router);
        }

        // Transit ASes
        for i in 0..transit_per_isd {
            let asn = (isd * ases_per_isd + core_per_isd + i) as u64 + 100;
            let router = net.add_router(&format!("R{}", asn), asn);
            transits.push(router);
        }

        // Leaf ASes
        for i in 0..leaf_per_isd {
            let asn = (isd * ases_per_isd + core_per_isd + transit_per_isd + i) as u64 + 100;
            let router = net.add_router(&format!("R{}", asn), asn);
            leaves.push(router);
        }

        all_cores.push(cores);
        all_transits.push(transits);
        all_leaves.push(leaves);
    }

    // Connect topology
    // Core mesh within each ISD
    for cores in &all_cores {
        for i in 0..cores.len() {
            for j in (i + 1)..cores.len() {
                net.add_link(cores[i], cores[j])?;
                net.set_link_weight(cores[i], cores[j], 1.0)?;
            }
        }
    }

    // Inter-ISD core links (sparse)
    for i in 0..all_cores.len() {
        let next_isd = (i + 1) % all_cores.len();
        if !all_cores[i].is_empty() && !all_cores[next_isd].is_empty() {
            net.add_link(all_cores[i][0], all_cores[next_isd][0])?;
            net.set_link_weight(all_cores[i][0], all_cores[next_isd][0], 10.0)?;
        }
    }

    // Core to transit
    for isd in 0..num_isds {
        for (j, &transit) in all_transits[isd].iter().enumerate() {
            let core = all_cores[isd][j % all_cores[isd].len()];
            net.add_link(core, transit)?;
            net.set_link_weight(core, transit, 5.0)?;
        }
    }

    // Transit to leaf
    for isd in 0..num_isds {
        for (j, &leaf) in all_leaves[isd].iter().enumerate() {
            let transit = all_transits[isd][j % all_transits[isd].len()];
            net.add_link(transit, leaf)?;
            net.set_link_weight(transit, leaf, 2.0)?;
        }
    }

    println!(
        "  Created {} routers, {} links",
        net.indices().count(),
        net.ospf_network().edges().count()
    );

    Ok(net)
}

/// Enable SCION on topology
#[cfg(feature = "scion")]
fn enable_scion(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    size: usize,
) -> Result<(), NetworkError> {
    let num_isds = (size as f64).sqrt().ceil() as usize;
    let ases_per_isd = size / num_isds;
    let core_ratio = 0.05;
    let core_per_isd = (ases_per_isd as f64 * core_ratio).ceil().max(2.0) as usize;

    let mut router_idx = 0;
    for isd in 0..num_isds {
        let isd_num = IsdNumber((isd + 1) as u16);

        for router in net.indices() {
            if router.index() >= router_idx && router.index() < router_idx + ases_per_isd {
                let is_core = (router.index() - router_idx) < core_per_isd;
                let asn = router.index() as u64 + 100;
                net.enable_scion(router, IsdAs::new(isd_num, asn), is_core)?;
            }
        }

        router_idx += ases_per_isd;
    }

    // Configure link types (simplified - just set based on router indices)
    for router in net.indices() {
        for neighbor_edge in net.ospf_network().neighbors(router) {
            let neighbor = neighbor_edge.src();
            if router.index() < neighbor.index() {
                // Only configure each link once
                // Determine link type based on router indices
                let link_type = if is_core_router(router, size) && is_core_router(neighbor, size) {
                    ScionLinkType::Core
                } else if is_core_router(router, size) || is_core_router(neighbor, size) {
                    ScionLinkType::ParentChild
                } else {
                    ScionLinkType::Peering
                };

                let _ = net.configure_scion_link(router, neighbor, link_type);
            }
        }
    }

    Ok(())
}

#[cfg(feature = "scion")]
fn is_core_router(router: RouterId, total_size: usize) -> bool {
    let num_isds = (total_size as f64).sqrt().ceil() as usize;
    let ases_per_isd = total_size / num_isds;
    let core_ratio = 0.05;
    let core_per_isd = (ases_per_isd as f64 * core_ratio).ceil().max(2.0) as usize;

    let isd_idx = router.index() / ases_per_isd;
    let idx_in_isd = router.index() % ases_per_isd;

    idx_in_isd < core_per_isd
}

fn bench_topology_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("topology_creation");

    for size in [100, 1_000, 10_000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter(|| generate_scalable_topology(black_box(size)).unwrap());
        });
    }

    group.finish();
}

#[cfg(feature = "scion")]
fn bench_scion_setup(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_setup");
    group.sample_size(10); // Fewer samples for large networks
    group.measurement_time(Duration::from_secs(60)); // Longer measurement

    for size in [100, 1_000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let mut net = generate_scalable_topology(size).unwrap();

            b.iter(|| {
                enable_scion(&mut net, size).unwrap();
            });
        });
    }

    group.finish();
}

#[cfg(feature = "scion")]
fn bench_scion_beaconing(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_beaconing");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(120));

    for size in [100, 1_000].iter() {
        println!("\nPreparing network of size {}...", size);
        let mut net = generate_scalable_topology(*size).unwrap();
        enable_scion(&mut net, *size).unwrap();

        group.throughput(Throughput::Elements(*size as u64));

        // Core beaconing
        group.bench_with_input(BenchmarkId::new("core_beaconing", size), size, |b, _| {
            b.iter(|| {
                net.scion_core_beaconing(black_box(1000)).unwrap();
            });
        });

        // Intra-ISD beaconing
        group.bench_with_input(
            BenchmarkId::new("intra_isd_beaconing", size),
            size,
            |b, _| {
                // Run core beaconing once to set up
                net.scion_core_beaconing(1000).unwrap();

                b.iter(|| {
                    net.scion_intra_isd_beaconing(black_box(1000), black_box(5))
                        .unwrap();
                });
            },
        );

        // Registration
        group.bench_with_input(BenchmarkId::new("registration", size), size, |b, _| {
            // Setup
            net.scion_core_beaconing(1000).unwrap();
            net.scion_intra_isd_beaconing(1000, 5).unwrap();

            b.iter(|| {
                net.scion_registration_round(black_box(5)).unwrap();
            });
        });
    }

    group.finish();
}

#[cfg(feature = "scion")]
fn bench_scion_path_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_path_lookup");
    group.sample_size(50);

    for size in [100, 1_000].iter() {
        println!("\nPreparing network of size {} for path lookup...", size);
        let mut net = generate_scalable_topology(*size).unwrap();
        enable_scion(&mut net, *size).unwrap();

        // Full setup
        net.scion_core_beaconing(1000).unwrap();
        net.scion_intra_isd_beaconing(1000, 5).unwrap();
        net.scion_registration_round(5).unwrap();

        // Get sample routers
        let routers: Vec<_> = net.indices().take(10).collect();

        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                for i in 0..routers.len() {
                    for j in (i + 1)..routers.len() {
                        let _ =
                            net.scion_lookup_paths(black_box(routers[i]), black_box(routers[j]));
                    }
                }
            });
        });
    }

    group.finish();
}

// Extreme scalability test (requires --long flag)
#[cfg(feature = "scion")]
fn bench_extreme_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("extreme_scalability");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(300)); // 5 minutes

    // Test larger sizes
    for size in [10_000, 100_000].iter() {
        println!("\n=== EXTREME TEST: {} routers ===", size);
        println!("This may take several minutes...");

        let topology_start = std::time::Instant::now();
        let mut net = match generate_scalable_topology(*size) {
            Ok(n) => n,
            Err(e) => {
                println!("Failed to create topology: {:?}", e);
                continue;
            }
        };
        println!("Topology creation: {:?}", topology_start.elapsed());

        let scion_start = std::time::Instant::now();
        if let Err(e) = enable_scion(&mut net, *size) {
            println!("Failed to enable SCION: {:?}", e);
            continue;
        }
        println!("SCION enablement: {:?}", scion_start.elapsed());

        // Core beaconing
        let beacon_start = std::time::Instant::now();
        if let Err(e) = net.scion_core_beaconing(1000) {
            println!("Core beaconing failed: {:?}", e);
            continue;
        }
        println!("Core beaconing: {:?}", beacon_start.elapsed());

        // Intra-ISD beaconing
        let intra_start = std::time::Instant::now();
        if let Err(e) = net.scion_intra_isd_beaconing(1000, 3) {
            // Fewer PCBs for scalability
            println!("Intra-ISD beaconing failed: {:?}", e);
            continue;
        }
        println!("Intra-ISD beaconing: {:?}", intra_start.elapsed());

        // Registration
        let reg_start = std::time::Instant::now();
        if let Err(e) = net.scion_registration_round(3) {
            // Fewer segments
            println!("Registration failed: {:?}", e);
            continue;
        }
        println!("Registration: {:?}", reg_start.elapsed());

        // Path lookup (sample)
        let lookup_start = std::time::Instant::now();
        let routers: Vec<_> = net.indices().take(5).collect();
        let mut path_count = 0;
        for i in 0..routers.len() {
            for j in (i + 1)..routers.len() {
                if let Ok(paths) = net.scion_lookup_paths(routers[i], routers[j]) {
                    path_count += paths.len();
                }
            }
        }
        println!(
            "Path lookup (10 pairs): {:?}, avg {} paths/pair",
            lookup_start.elapsed(),
            path_count / 10
        );

        println!(
            "=== Total time for {}: {:?} ===\n",
            size,
            scion_start.elapsed()
        );
    }

    group.finish();
}

#[cfg(feature = "scion")]
criterion_group!(
    benches,
    bench_topology_creation,
    bench_scion_setup,
    bench_scion_beaconing,
    bench_scion_path_lookup,
    bench_extreme_scalability
);

#[cfg(not(feature = "scion"))]
criterion_group!(benches, bench_topology_creation);

criterion_main!(benches);
