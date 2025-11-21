// SPDX-License-Identifier: GPL-2.0-or-later
//
// SCION Static-Mode Scalability Evaluation
//
// This benchmark exercises the event-driven SCION control plane using the new
// static simulation mode.  Each AS is provisioned with four border routers on
// average, and beaconing is performed with a single wave per ISD plus a
// diameter-bounded inter-ISD core propagation.  The program prints metrics to
// stdout and appends a CSV row for each test size to
// `scion_static_scaling.csv`.  Use `examples/scion_static_scaling_plot.py` to
// turn the CSV into plots.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

use bgpsim::{
    event::BasicEventQueue,
    ospf::GlobalOspf,
    prelude::*,
    scion::{InterfaceId, IsdAs, ScionLinkType, ScionSimulationMode},
    types::{RouterId, SimplePrefix, ASN},
};

const ROUTERS_PER_AS: usize = 4;
const CORE_RATIO: f64 = 0.08;
const TRANSIT_RATIO: f64 = 0.22;

fn main() -> Result<(), NetworkError> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .try_init();
    let sizes: Vec<usize> = parse_sizes();
    let csv_path = Path::new("scion_static_scaling.csv");

    ensure_csv_header(csv_path).expect("failed to create csv header");

    for ases in sizes {
        println!(
            "\n==================== Evaluating {} ASes ({} routers) ====================",
            ases,
            ases * ROUTERS_PER_AS
        );

        let (metrics, csv_row) = run_single_scale(ases)?;
        println!("{}", metrics);

        append_csv(csv_path, &csv_row).expect("failed writing csv row");
    }

    println!("\nCSV written to scion_static_scaling.csv");
    println!("Use `python examples/scion_static_scaling_plot.py scion_static_scaling.csv` to render plots.");
    Ok(())
}

fn parse_sizes() -> Vec<usize> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        vec![200, 800, 1600, 3200]
    } else {
        args.into_iter()
            .filter_map(|a| a.parse::<usize>().ok())
            .collect::<Vec<_>>()
    }
}

fn run_single_scale(total_ases: usize) -> Result<(String, String), NetworkError> {
    let topology_start = Instant::now();
    let (mut net, topo_meta) = build_topology(total_ases)?;
    let topology_time = topology_start.elapsed();

    net.set_scion_simulation_mode(ScionSimulationMode::Static)?;
    println!("  > SCION mode set to {:?}", net.scion_simulation_mode());

    let beacon_start = Instant::now();
    println!("  > Starting event-driven beaconing…");
    net.scion_start_beaconing(1000)?;
    let events_processed = net.scion_converge()?;
    let beacon_time = beacon_start.elapsed();
    println!("    · Event-driven beaconing done (events {events_processed})");

    let reg_limit = net.scion_simulation_mode().up_down_segment_limit();
    let reg_start = Instant::now();
    println!("  > Starting registration (limit {reg_limit})…");
    let (registered_up, registered_down, registered_core) =
        net.scion_registration_round(reg_limit)?;
    let reg_time = reg_start.elapsed();
    println!("    · Registration done (up {registered_up}, down {registered_down}, core {registered_core})");

    let (total_pcbs, total_segments) = collect_control_plane_counters(&net, &topo_meta.all_as_ids);

    let (lookup_count, avg_lookup_time, min_path_count, max_path_count, min_pair) =
        run_lookup_sampling(&net, &topo_meta.sample_targets)?;

    // Diagnose low path counts (likely issue in larger topologies)
    if min_path_count < 50 && topo_meta.all_as_ids.len() >= 80 {
        eprintln!(
            "\n⚠️  Low path count detected ({} paths for {} ASes), running diagnosis...",
            min_path_count,
            topo_meta.all_as_ids.len()
        );
        if let Some((src, dst)) = min_pair {
            if let Err(e) = diagnose_path_lookup(&net, src, dst) {
                eprintln!("Diagnosis error: {:?}", e);
            }
        } else if topo_meta.sample_targets.len() >= 2 {
            if let Err(e) = diagnose_path_lookup(
                &net,
                topo_meta.sample_targets[0],
                topo_meta.sample_targets[1],
            ) {
                eprintln!("Diagnosis error: {:?}", e);
            }
        }
    }

    let total_time = topology_time + beacon_time + reg_time;

    let metrics = format!(
        "Topology: {} ISDs, avg {:.2} AS/ISD (cores {}, transits {}, leaves {})\n\
         Timings: topo {:.3}s, beacon {:.3}s, registration {:.3}s, total {:.3}s\n\
         Control plane: events {}, PCBs {}, segments {}, registered (up {}, down {}, core {})\n\
         Lookup: {} samples, avg {:.6}s, min paths {}, max paths {}",
        topo_meta.num_isds,
        topo_meta.avg_ases_per_isd(),
        topo_meta.cores_per_isd,
        topo_meta.transits_per_isd,
        topo_meta.leaves_per_isd,
        topology_time.as_secs_f64(),
        beacon_time.as_secs_f64(),
        reg_time.as_secs_f64(),
        total_time.as_secs_f64(),
        events_processed,
        total_pcbs,
        total_segments,
        registered_up,
        registered_down,
        registered_core,
        lookup_count,
        avg_lookup_time.as_secs_f64(),
        min_path_count,
        max_path_count
    );

    let csv_row = format!(
        "{ases},{routers},{isds},{cores},{transits},{leaves},{routers_per_as},{topo:.6},{beacon:.6},{reg:.6},{total:.6},{events},{pcbs},{segments},{lookup_count},{lookup_avg:.9},{min_paths},{max_paths}",
        ases = total_ases,
        routers = total_ases * ROUTERS_PER_AS,
        isds = topo_meta.num_isds,
        cores = topo_meta.cores_per_isd,
        transits = topo_meta.transits_per_isd,
        leaves = topo_meta.leaves_per_isd,
        routers_per_as = ROUTERS_PER_AS,
        topo = topology_time.as_secs_f64(),
        beacon = beacon_time.as_secs_f64(),
        reg = reg_time.as_secs_f64(),
        total = total_time.as_secs_f64(),
        events = events_processed,
        pcbs = total_pcbs,
        segments = total_segments,
        lookup_count = lookup_count,
        lookup_avg = avg_lookup_time.as_secs_f64(),
        min_paths = min_path_count,
        max_paths = max_path_count
    );

    Ok((metrics, csv_row))
}

fn diagnose_path_lookup(
    net: &Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    src: IsdAs,
    dst: IsdAs,
) -> Result<(), NetworkError> {
    eprintln!("\n=== Path Lookup Diagnosis: {:?} -> {:?} ===", src, dst);

    // Get AS-level info
    let src_as = net
        .get_scion_as(&src)
        .ok_or_else(|| NetworkError::DeviceNotFound(RouterId::from(0)))?;
    let dst_as = net
        .get_scion_as(&dst)
        .ok_or_else(|| NetworkError::DeviceNotFound(RouterId::from(0)))?;

    eprintln!("Source AS: {:?} (core: {})", src, src_as.is_core);
    eprintln!("Destination AS: {:?} (core: {})", dst, dst_as.is_core);

    // Count registered segments
    let src_up = src_as
        .control_service
        .path_database
        .get_all_up_segments()
        .len();
    let src_down = src_as
        .control_service
        .path_database
        .get_all_down_segments()
        .len();
    let src_core = src_as
        .control_service
        .path_database
        .get_all_core_segments()
        .len();
    eprintln!(
        "Source AS segments: {} up, {} down, {} core",
        src_up, src_down, src_core
    );
    if let Some(parent_info) = net
        .get_scion_as(&src)
        .map(|as_entry| as_entry.control_service.get_parent_interfaces())
    {
        eprintln!("Source parent interfaces: {}", parent_info.len());
        for iface in parent_info.iter().take(4) {
            eprintln!(
                "  · parent via {:?} link (iface {:?}) to {:?}",
                iface.link_type, iface.interface_id, iface.neighbor_isd_as
            );
        }
        if parent_info.len() > 4 {
            eprintln!("  ... {} more parents", parent_info.len() - 4);
        }
    }

    let dst_up = dst_as
        .control_service
        .path_database
        .get_all_up_segments()
        .len();
    let dst_down = dst_as
        .control_service
        .path_database
        .get_all_down_segments()
        .len();
    let dst_core = dst_as
        .control_service
        .path_database
        .get_all_core_segments()
        .len();
    eprintln!(
        "Destination AS segments: {} up, {} down, {} core",
        dst_up, dst_down, dst_core
    );
    if let Some(parent_info) = net
        .get_scion_as(&dst)
        .map(|as_entry| as_entry.control_service.get_parent_interfaces())
    {
        eprintln!("Destination parent interfaces: {}", parent_info.len());
        for iface in parent_info.iter().take(4) {
            eprintln!(
                "  · parent via {:?} link (iface {:?}) to {:?}",
                iface.link_type, iface.interface_id, iface.neighbor_isd_as
            );
        }
        if parent_info.len() > 4 {
            eprintln!("  ... {} more parents", parent_info.len() - 4);
        }
    }
    if let Some(dst_entry) = net.get_scion_as(&dst) {
        let beacon_total = dst_entry.control_service.beacon_store.total_count();
        let beacon_sources = dst_entry.control_service.beacon_store.source_count();
        eprintln!(
            "Destination beacon store entries: {} (from {} sources)",
            beacon_total, beacon_sources
        );
    }

    if src.isd != dst.isd {
        // Inter-ISD lookup
        eprintln!("\nInter-ISD path lookup:");

        // Find core ASes by iterating over routers
        let src_cores: Vec<_> = net
            .routers()
            .filter_map(|r| {
                r.scion()
                    .filter(|cs| cs.is_core && cs.isd_as.isd == src.isd)
                    .map(|cs| cs.isd_as)
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        let dst_cores: Vec<_> = net
            .routers()
            .filter_map(|r| {
                r.scion()
                    .filter(|cs| cs.is_core && cs.isd_as.isd == dst.isd)
                    .map(|cs| cs.isd_as)
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        eprintln!("  Source ISD cores: {} ({:?})", src_cores.len(), src_cores);
        eprintln!(
            "  Destination ISD cores: {} ({:?})",
            dst_cores.len(),
            dst_cores
        );

        // Check up segments from source
        if !src_as.is_core {
            for src_core_as in &src_cores {
                let up_segs = src_as.control_service.lookup_up_segments(src_core_as);
                eprintln!(
                    "  Up segments from {:?} to {:?}: {} segments",
                    src,
                    src_core_as,
                    up_segs.len()
                );
                if up_segs.len() > 0 {
                    eprintln!(
                        "    First segment destination: {:?}",
                        up_segs[0].destination()
                    );
                }
            }
        }

        // Check down segments registered at destination ISD cores
        if !dst_as.is_core {
            let mut total_down = 0;
            for core in &dst_cores {
                if let Some(core_as) = net.get_scion_as(core) {
                    let segs = core_as.control_service.lookup_down_segments_to(&dst);
                    total_down += segs.len();
                    eprintln!(
                        "  Down segments from core {:?} to {:?}: {}",
                        core,
                        dst,
                        segs.len()
                    );
                    if let Some(seg) = segs.first() {
                        eprintln!("    First segment destination: {:?}", seg.destination());
                    }
                }
            }
            eprintln!("  Total down segments to {:?}: {}", dst, total_down);
        }
    } else {
        // Intra-ISD lookup
        eprintln!("\nIntra-ISD path lookup:");

        // Find core ASes
        let cores: Vec<_> = net
            .routers()
            .filter_map(|r| {
                r.scion()
                    .filter(|cs| cs.is_core && cs.isd_as.isd == src.isd)
                    .map(|cs| cs.isd_as)
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        eprintln!("  Core ASes: {} ({:?})", cores.len(), cores);

        if !src_as.is_core {
            for core_as in &cores {
                let up_segs = src_as.control_service.lookup_up_segments(core_as);
                eprintln!(
                    "  Up segments from {:?} to {:?}: {} segments",
                    src,
                    core_as,
                    up_segs.len()
                );
            }
        }

        if !dst_as.is_core {
            let mut total_down = 0;
            for core in &cores {
                if let Some(core_as) = net.get_scion_as(core) {
                    let segs = core_as.control_service.lookup_down_segments_to(&dst);
                    total_down += segs.len();
                    eprintln!(
                        "  Down segments from core {:?} to {:?}: {}",
                        core,
                        dst,
                        segs.len()
                    );
                }
            }
            eprintln!("  Total down segments to {:?}: {}", dst, total_down);
        }
    }

    // Perform actual lookup using IsdAs directly
    let paths = net.scion_lookup_paths(src, dst)?;
    eprintln!("\nActual path lookup result: {} paths", paths.len());
    if paths.len() > 0 {
        eprintln!("  First path AS sequence: {:?}", paths[0].as_path);
        if paths.len() <= 10 {
            for (i, path) in paths.iter().enumerate() {
                eprintln!("    Path {}: {:?}", i, path.as_path);
            }
        }
    }

    eprintln!("=== End Diagnosis ===\n");
    Ok(())
}

fn run_lookup_sampling(
    net: &Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    sample_targets: &[IsdAs],
) -> Result<(usize, Duration, usize, usize, Option<(IsdAs, IsdAs)>), NetworkError> {
    if sample_targets.len() < 2 {
        return Ok((0, Duration::from_secs(0), 0, 0, None));
    }

    let mut total = Duration::from_secs(0);
    let mut count = 0;
    let mut min_paths = usize::MAX;
    let mut max_paths = 0;
    let mut min_pair = None;
    let mut total_paths = 0;
    let mut total_unique_paths = 0;
    let mut total_as_level_duplicates = 0;
    let mut max_as_duplicates = 0;
    let mut max_full_duplicates = 0;
    let mut lookup_with_max_as_duplicates = None;
    let mut lookup_with_max_full_duplicates = None;
    let mut lookup_with_256_paths = None;

    let sample_count = sample_targets.len().min(6);
    for i in 0..sample_count {
        for j in (i + 1)..sample_count {
            let start = Instant::now();
            let paths = net.scion_lookup_paths(sample_targets[i], sample_targets[j])?;
            let elapsed = start.elapsed();
            total += elapsed;
            count += 1;
            let path_count = paths.len();
            total_paths += path_count;
            min_paths = min_paths.min(path_count);
            max_paths = max_paths.max(path_count);
            if path_count <= min_paths {
                min_pair = Some((sample_targets[i], sample_targets[j]));
            }

            // Check for duplicates at two levels:
            // 1. AS-level duplicates (same AS path, different interfaces)
            // 2. Full-path duplicates (same AS path AND same interfaces)
            use std::collections::HashSet;

            #[derive(Hash, PartialEq, Eq)]
            struct PathSignature {
                as_path: Vec<IsdAs>,
                interfaces: Vec<(InterfaceId, InterfaceId)>,
            }

            let mut unique_as_paths = HashSet::new();
            let mut unique_full_paths = HashSet::new();

            for path in &paths {
                // AS-level comparison
                unique_as_paths.insert(&path.as_path);

                // Full path comparison (AS path + interface IDs)
                let hop_fields = path.get_hop_fields();
                let signature = PathSignature {
                    as_path: path.as_path.clone(),
                    interfaces: hop_fields
                        .iter()
                        .map(|hf| (hf.ingress, hf.egress))
                        .collect(),
                };
                unique_full_paths.insert(signature);
            }

            let unique_as_count = unique_as_paths.len();
            let unique_full_count = unique_full_paths.len();
            let as_level_duplicates = path_count.saturating_sub(unique_as_count);
            let full_path_duplicates = path_count.saturating_sub(unique_full_count);

            total_unique_paths += unique_full_count;
            total_as_level_duplicates += as_level_duplicates;

            if as_level_duplicates > max_as_duplicates {
                max_as_duplicates = as_level_duplicates;
                lookup_with_max_as_duplicates = Some((
                    sample_targets[i],
                    sample_targets[j],
                    path_count,
                    as_level_duplicates,
                    unique_as_count,
                ));
            }

            if full_path_duplicates > max_full_duplicates {
                max_full_duplicates = full_path_duplicates;
                lookup_with_max_full_duplicates = Some((
                    sample_targets[i],
                    sample_targets[j],
                    path_count,
                    full_path_duplicates,
                ));
            }

            // Track a lookup that returns exactly 256 paths (common in small topologies)
            if path_count == 256 && lookup_with_256_paths.is_none() {
                lookup_with_256_paths = Some((
                    sample_targets[i],
                    sample_targets[j],
                    unique_as_count,
                    as_level_duplicates,
                    unique_full_count,
                    full_path_duplicates,
                ));
            }
        }
    }

    let avg = if count > 0 {
        total / count as u32
    } else {
        Duration::from_secs(0)
    };

    if count == 0 {
        min_paths = 0;
    }

    // Report duplicate statistics
    let total_full_duplicates = total_paths.saturating_sub(total_unique_paths);
    let as_duplicate_percentage = if total_paths > 0 {
        (total_as_level_duplicates as f64 / total_paths as f64) * 100.0
    } else {
        0.0
    };
    let full_duplicate_percentage = if total_paths > 0 {
        (total_full_duplicates as f64 / total_paths as f64) * 100.0
    } else {
        0.0
    };

    eprintln!(
        "Path duplicate analysis (across {} lookups, {} total paths):",
        count, total_paths
    );
    eprintln!(
        "  AS-level duplicates: {} ({:.1}%) - paths with same AS sequence but different interfaces",
        total_as_level_duplicates, as_duplicate_percentage
    );
    eprintln!(
        "  Full-path duplicates: {} ({:.1}%) - paths with same AS sequence AND same interfaces",
        total_full_duplicates, full_duplicate_percentage
    );
    eprintln!(
        "  Unique full paths: {} ({:.1}%)",
        total_unique_paths,
        100.0 - full_duplicate_percentage
    );

    if let Some((src, dst, total, as_dup, as_unique)) = lookup_with_max_as_duplicates {
        eprintln!(
            "  Worst AS-level duplicates: {} out of {} paths ({} unique AS paths, {:.1}%, src={:?}, dst={:?})",
            as_dup, total, as_unique, (as_dup as f64 / total as f64) * 100.0, src, dst
        );
    }

    if let Some((src, dst, total, full_dup)) = lookup_with_max_full_duplicates {
        eprintln!(
            "  Worst full-path duplicates: {} out of {} paths ({:.1}%, src={:?}, dst={:?})",
            full_dup,
            total,
            (full_dup as f64 / total as f64) * 100.0,
            src,
            dst
        );
    }

    if let Some((src, dst, as_unique, as_dup, full_unique, full_dup)) = lookup_with_256_paths {
        eprintln!("  256-path case (src={:?}, dst={:?}):", src, dst);
        eprintln!(
            "    AS-level: {} unique AS paths, {} AS-level duplicates ({:.1}%)",
            as_unique,
            as_dup,
            (as_dup as f64 / 256.0) * 100.0
        );
        eprintln!(
            "    Full-path: {} unique full paths, {} full-path duplicates ({:.1}%)",
            full_unique,
            full_dup,
            (full_dup as f64 / 256.0) * 100.0
        );
    }

    Ok((count, avg, min_paths, max_paths, min_pair))
}

fn collect_control_plane_counters(
    net: &Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    as_ids: &[IsdAs],
) -> (usize, usize) {
    let mut pcbs = 0;
    let mut segments = 0;
    for isd_as in as_ids {
        if let Some(as_ref) = net.get_scion_as(isd_as) {
            pcbs += as_ref.control_service.beacon_store.total_count();
            segments += as_ref.control_service.path_database.total_count();
        }
    }
    (pcbs, segments)
}

fn ensure_csv_header(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    let mut file = File::create(path)?;
    writeln!(
        file,
        "ases,routers,isds,cores_per_isd,transits_per_isd,leaves_per_isd,routers_per_as,topology_time_s,beacon_time_s,registration_time_s,total_time_s,events_processed,pcbs_total,segments_total,lookup_samples,avg_lookup_s,min_path_count,max_path_count"
    )
}

fn append_csv(path: &Path, row: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{row}")
}

struct TopologyMeta {
    num_isds: usize,
    cores_per_isd: usize,
    transits_per_isd: usize,
    leaves_per_isd: usize,
    all_as_ids: Vec<IsdAs>,
    sample_targets: Vec<IsdAs>,
}

impl TopologyMeta {
    fn avg_ases_per_isd(&self) -> f64 {
        (self.cores_per_isd + self.transits_per_isd + self.leaves_per_isd) as f64
    }
}

struct AsHandle {
    isd_as: IsdAs,
    routers: Vec<RouterId>,
    next_router: usize,
    is_core: bool,
}

impl AsHandle {
    fn new(
        net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
        isd_number: u16,
        local_idx: usize,
        is_core: bool,
    ) -> Result<Self, NetworkError> {
        let base_asn = ((isd_number as u32) << 16) | (local_idx as u32 + 100);
        let isd_as = IsdAs::new(isd_number, base_asn as u64);
        let mut routers = Vec::with_capacity(ROUTERS_PER_AS);
        for r_idx in 0..ROUTERS_PER_AS {
            let router = net.add_router(
                &format!(
                    "ISD{}-{}-BR{}",
                    isd_number,
                    if is_core { "Core" } else { "AS" },
                    r_idx
                ),
                ASN(base_asn),
            );
            net.enable_scion(router, isd_as, is_core)?;
            routers.push(router);
        }
        Ok(Self {
            isd_as,
            routers,
            next_router: 0,
            is_core,
        })
    }

    fn grab_router(&mut self) -> RouterId {
        let router = self.routers[self.next_router % self.routers.len()];
        self.next_router = (self.next_router + 1) % self.routers.len();
        router
    }
}

fn connect_ases(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    a: &mut AsHandle,
    b: &mut AsHandle,
    link_type: ScionLinkType,
) -> Result<(), NetworkError> {
    let router_a = a.grab_router();
    let router_b = b.grab_router();
    net.add_link(router_a, router_b)?;
    net.configure_scion_link(router_a, router_b, link_type)?;
    Ok(())
}

fn build_topology(
    total_ases: usize,
) -> Result<
    (
        Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
        TopologyMeta,
    ),
    NetworkError,
> {
    let mut net = Network::default();
    let num_isds = (total_ases as f64).sqrt().ceil().max(1.0) as usize;
    let ases_per_isd = (total_ases / num_isds).max(3);

    let mut all_as_ids = Vec::new();
    let mut sample_targets = Vec::new();

    let mut per_isd_core = 0usize;
    let mut per_isd_transit = 0usize;
    let mut per_isd_leaf = 0usize;

    let mut isd_handles: Vec<Vec<AsHandle>> = Vec::with_capacity(num_isds);

    println!(
        "  > Topology target: {} ISDs, {} AS/ISD (cores ~{:.1}%, transits ~{:.1}%)",
        num_isds,
        ases_per_isd,
        CORE_RATIO * 100.0,
        TRANSIT_RATIO * 100.0
    );

    for isd_idx in 0..num_isds {
        let isd_number = (isd_idx + 1) as u16;
        let mut cores = Vec::new();
        let mut transits = Vec::new();
        let mut leaves = Vec::new();

        let mut core_count = (ases_per_isd as f64 * CORE_RATIO).ceil() as usize;
        core_count = core_count.clamp(2, ases_per_isd);

        let mut transit_count = (ases_per_isd as f64 * TRANSIT_RATIO).ceil() as usize;
        transit_count = transit_count.max(1);

        let mut leaf_count = ases_per_isd.saturating_sub(core_count + transit_count);
        if leaf_count == 0 {
            leaf_count = 1;
            if transit_count > 1 {
                transit_count -= 1;
            }
        }

        println!(
            "    - ISD {}: cores {}, transits {}, leaves {}",
            isd_number, core_count, transit_count, leaf_count
        );

        per_isd_core = core_count;
        per_isd_transit = transit_count;
        per_isd_leaf = leaf_count;

        let mut local_idx = 0;
        for _ in 0..core_count {
            let handle = AsHandle::new(&mut net, isd_number, local_idx, true)?;
            all_as_ids.push(handle.isd_as);
            cores.push(handle);
            local_idx += 1;
        }

        for _ in 0..transit_count {
            let handle = AsHandle::new(&mut net, isd_number, local_idx, false)?;
            all_as_ids.push(handle.isd_as);
            transits.push(handle);
            local_idx += 1;
        }

        for _ in 0..leaf_count {
            let handle = AsHandle::new(&mut net, isd_number, local_idx, false)?;
            sample_targets.push(handle.isd_as);
            all_as_ids.push(handle.isd_as);
            leaves.push(handle);
            local_idx += 1;
        }

        connect_core_mesh(&mut net, &mut cores)?;
        connect_tiers(&mut net, &mut cores, &mut transits)?;
        connect_tiers(&mut net, &mut transits, &mut leaves)?;
        println!("      · ISD {} wiring complete", isd_number);

        let mut tier_handles = Vec::new();
        tier_handles.extend(cores.into_iter());
        tier_handles.extend(transits.into_iter());
        tier_handles.extend(leaves.into_iter());
        isd_handles.push(tier_handles);
    }

    println!("  > Connecting ISDs (ring)...");
    connect_isd_ring(&mut net, &mut isd_handles)?;
    println!("  > Topology wiring complete");

    let meta = TopologyMeta {
        num_isds,
        cores_per_isd: per_isd_core,
        transits_per_isd: per_isd_transit,
        leaves_per_isd: per_isd_leaf,
        all_as_ids,
        sample_targets,
    };

    Ok((net, meta))
}

fn connect_core_mesh(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    cores: &mut [AsHandle],
) -> Result<(), NetworkError> {
    for i in 0..cores.len() {
        for j in (i + 1)..cores.len() {
            let (left, right) = cores.split_at_mut(j);
            let a = &mut left[i];
            let b = &mut right[0];
            connect_ases(net, a, b, ScionLinkType::Core)?;
        }
    }
    Ok(())
}

fn connect_tiers(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    parents: &mut [AsHandle],
    children: &mut [AsHandle],
) -> Result<(), NetworkError> {
    if parents.is_empty() {
        return Ok(());
    }
    for (idx, child) in children.iter_mut().enumerate() {
        let parent_idx = idx % parents.len();
        let (left, _right) = parents.split_at_mut(parent_idx + 1);
        let parent = &mut left[left.len() - 1];
        connect_ases(net, parent, child, ScionLinkType::ParentChild)?;
    }
    Ok(())
}

fn connect_isd_ring(
    net: &mut Network<SimplePrefix, BasicEventQueue<SimplePrefix>, GlobalOspf>,
    isd_handles: &mut [Vec<AsHandle>],
) -> Result<(), NetworkError> {
    if isd_handles.len() < 2 {
        return Ok(());
    }
    for i in 0..isd_handles.len() {
        let next = (i + 1) % isd_handles.len();
        if i == next {
            continue;
        }
        let (current_vec, next_vec) = split_two_mut(isd_handles, i, next);
        let current_core = current_vec
            .iter_mut()
            .find(|handle| handle.is_core)
            .ok_or(NetworkError::DeviceNotFound(RouterId::from(0u32)))?;
        let next_core = next_vec
            .iter_mut()
            .find(|handle| handle.is_core)
            .ok_or(NetworkError::DeviceNotFound(RouterId::from(0u32)))?;
        connect_ases(net, current_core, next_core, ScionLinkType::Core)?;
    }
    Ok(())
}

fn split_two_mut<T>(slice: &mut [T], idx_a: usize, idx_b: usize) -> (&mut T, &mut T) {
    assert!(idx_a != idx_b);
    if idx_a < idx_b {
        let (left, right) = slice.split_at_mut(idx_b);
        (&mut left[idx_a], &mut right[0])
    } else {
        let (left, right) = slice.split_at_mut(idx_a);
        (&mut right[0], &mut left[idx_b])
    }
}
