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

//! Benchmarks for SCION control plane operations

use bgpsim::prelude::*;
use bgpsim::event::BasicEventQueue;
use bgpsim::ospf::GlobalOspf;
use bgpsim::scion::{IsdAs, ScionLinkType};
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

/// Create a simple core network with N core ASes in a ring topology
fn create_core_network(n: usize) -> Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf> {
    let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

    let mut cores = Vec::new();
    for i in 0..n {
        let router = net.add_router(&format!("Core{}", i), 65500 + i as u32);
        net.enable_scion(router, IsdAs::new(1, (110 + i) as u64), true).unwrap();
        cores.push(router);
    }

    // Connect cores in a ring
    for i in 0..n {
        let next = (i + 1) % n;
        net.add_link(cores[i], cores[next]).unwrap();
        net.configure_scion_link(cores[i], cores[next], ScionLinkType::Core).unwrap();
    }

    net
}

/// Create a hierarchical network: 1 core with N non-core ASes
fn create_hierarchical_network(n_children: usize) -> Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf> {
    let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

    let core = net.add_router("Core", 65500);
    net.enable_scion(core, IsdAs::new(1, 110u64), true).unwrap();

    for i in 0..n_children {
        let child = net.add_router(&format!("AS{}", i), 65501 + i as u32);
        net.enable_scion(child, IsdAs::new(1, (111 + i) as u64), false).unwrap();
        net.add_link(core, child).unwrap();
        net.configure_scion_link(core, child, ScionLinkType::ParentChild).unwrap();
    }

    net
}

/// Create a multi-ISD network
fn create_multi_isd_network(n_isds: usize) -> Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf> {
    let mut net: Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf> = Network::default();

    let mut cores = Vec::new();
    for i in 0..n_isds {
        let core = net.add_router(&format!("Core{}", i), 65500 + i as u32);
        net.enable_scion(core, IsdAs::new((i + 1) as u16, ((i + 1) * 110) as u64), true).unwrap();
        cores.push(core);
    }

    // Connect ISDs in a chain
    for i in 0..(n_isds - 1) {
        net.add_link(cores[i], cores[i + 1]).unwrap();
        net.configure_scion_link(cores[i], cores[i + 1], ScionLinkType::Core).unwrap();
    }

    net
}

fn benchmark_core_beaconing(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_core_beaconing");

    for size in [3, 5, 10, 20].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_batched(
                || create_core_network(size),
                |mut net| {
                    net.scion_core_beaconing(1000).unwrap();
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn benchmark_intra_isd_beaconing(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_intra_isd_beaconing");

    for size in [5, 10, 20, 50].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_batched(
                || {
                    let mut net = create_hierarchical_network(size);
                    net.scion_core_beaconing(1000).unwrap();
                    net
                },
                |mut net| {
                    net.scion_intra_isd_beaconing(1000, 5).unwrap();
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn benchmark_registration(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_registration");

    for size in [5, 10, 20, 50].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_batched(
                || {
                    let mut net = create_hierarchical_network(size);
                    net.scion_core_beaconing(1000).unwrap();
                    net.scion_intra_isd_beaconing(1000, 5).unwrap();
                    net
                },
                |mut net| {
                    net.scion_registration_round(5).unwrap();
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn benchmark_intra_isd_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_intra_isd_lookup");

    for size in [5, 10, 20, 50].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_batched(
                || {
                    let mut net = create_hierarchical_network(size);
                    net.scion_core_beaconing(1000).unwrap();
                    net.scion_intra_isd_beaconing(1000, 5).unwrap();
                    net.scion_registration_round(5).unwrap();

                    // Get source and destination routers
                    let routers: Vec<_> = net.routers()
                        .filter(|r| r.scion().map(|cs| !cs.is_core).unwrap_or(false))
                        .map(|r| r.router_id())
                        .collect();

                    (net, routers[0], routers[routers.len() - 1])
                },
                |(net, src, dst)| {
                    net.scion_lookup_paths(src, dst).unwrap();
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn benchmark_inter_isd_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_inter_isd_lookup");

    for size in [3, 5, 7, 10].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_batched(
                || {
                    let mut net = create_multi_isd_network(size);
                    net.scion_core_beaconing(1000).unwrap();
                    net.scion_registration_round(5).unwrap();

                    // Get first and last core routers
                    let routers: Vec<_> = net.routers()
                        .map(|r| r.router_id())
                        .collect();

                    (net, routers[0], routers[routers.len() - 1])
                },
                |(net, src, dst)| {
                    net.scion_lookup_paths(src, dst).unwrap();
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

fn benchmark_complete_workflow(c: &mut Criterion) {
    let mut group = c.benchmark_group("scion_complete_workflow");

    for size in [5, 10, 20].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            b.iter_batched(
                || create_hierarchical_network(size),
                |mut net| {
                    net.scion_core_beaconing(1000).unwrap();
                    net.scion_intra_isd_beaconing(1000, 5).unwrap();
                    net.scion_registration_round(5).unwrap();

                    // Get source and destination
                    let routers: Vec<_> = net.routers()
                        .filter(|r| r.scion().map(|cs| !cs.is_core).unwrap_or(false))
                        .map(|r| r.router_id())
                        .collect();

                    if routers.len() >= 2 {
                        net.scion_lookup_paths(routers[0], routers[routers.len() - 1]).unwrap();
                    }
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(
    scion_benches,
    benchmark_core_beaconing,
    benchmark_intra_isd_beaconing,
    benchmark_registration,
    benchmark_intra_isd_lookup,
    benchmark_inter_isd_lookup,
    benchmark_complete_workflow,
);

criterion_main!(scion_benches);
