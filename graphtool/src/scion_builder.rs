//! Build a SCION-enabled bgpsim Network from a graph-tool graph.
//!
//! This module provides efficient conversion from a loaded GtGraph to a fully
//! configured SCION network, ready for beaconing simulation.

use std::collections::{BTreeMap, BTreeSet};

use bgpsim::event::BasicEventQueue;
use bgpsim::network::Network;
use bgpsim::ospf::MinimalOspf;
use bgpsim::scion::{IsdAs, ScionLinkType};
use bgpsim::types::{RouterId, SimplePrefix};

use crate::types::GtGraph;
use crate::GraphToolError;

/// Type alias for SCION network using MinimalOspf for efficiency.
pub type ScionNetwork = Network<SimplePrefix, BasicEventQueue<SimplePrefix>, MinimalOspf>;

/// Statistics about the built network.
#[derive(Debug, Clone, Default)]
pub struct BuildStats {
    /// Number of ISDs
    pub isd_count: usize,
    /// Number of unique ASes
    pub as_count: usize,
    /// Number of core ASes
    pub core_as_count: usize,
    /// Number of routers (nodes)
    pub router_count: usize,
    /// Number of physical links
    pub link_count: usize,
    /// Number of inter-AS SCION links
    pub scion_link_count: usize,
    /// Number of intra-AS links
    pub intra_as_link_count: usize,
}

/// Builder for constructing a SCION network from a GtGraph.
pub struct ScionNetworkBuilder {
    graph: GtGraph,
    /// Mapping from original node ID to RouterId
    node_to_router: BTreeMap<u32, RouterId>,
    /// Mapping from ISD-AS to list of routers in that AS
    as_to_routers: BTreeMap<IsdAs, Vec<RouterId>>,
    /// Mapping from ISD-AS to whether it's a core AS
    as_is_core: BTreeMap<IsdAs, bool>,
    /// Build statistics
    stats: BuildStats,
}

impl ScionNetworkBuilder {
    /// Create a new builder from a loaded graph.
    pub fn new(graph: GtGraph) -> Self {
        Self {
            graph,
            node_to_router: BTreeMap::new(),
            as_to_routers: BTreeMap::new(),
            as_is_core: BTreeMap::new(),
            stats: BuildStats::default(),
        }
    }

    /// Build the SCION network.
    ///
    /// This performs the following steps:
    /// 1. Analyze graph to identify ASes and their properties
    /// 2. Create routers for each node
    /// 3. Enable SCION for each router
    /// 4. Add physical links (in batch for efficiency)
    /// 5. Add SCION link metadata for inter-AS links
    ///
    /// # Returns
    /// A tuple of (Network, BuildStats)
    pub fn build(mut self) -> Result<(ScionNetwork, BuildStats), GraphToolError> {
        // Phase 1: Analyze ASes
        self.analyze_ases();

        // Phase 2: Create network and add routers
        let mut net = self.create_network_with_routers()?;

        // Phase 3: Add physical links in batch
        self.add_physical_links(&mut net)?;

        // Phase 4: Add SCION links for inter-AS edges
        self.add_scion_links(&mut net)?;

        Ok((net, self.stats))
    }

    /// Analyze ASes from the graph to determine core status.
    fn analyze_ases(&mut self) {
        let mut isd_set: BTreeSet<u16> = BTreeSet::new();
        let mut as_set: BTreeSet<IsdAs> = BTreeSet::new();
        let mut as_core: BTreeMap<IsdAs, bool> = BTreeMap::new();

        for node in &self.graph.nodes {
            let isd_as = IsdAs::new(node.isd, node.asn);
            isd_set.insert(node.isd);
            as_set.insert(isd_as);

            // An AS is core if ANY of its routers has core=true
            let entry = as_core.entry(isd_as).or_insert(false);
            if node.core {
                *entry = true;
            }
        }

        self.as_is_core = as_core.clone();
        self.stats.isd_count = isd_set.len();
        self.stats.as_count = as_set.len();
        self.stats.core_as_count = as_core.values().filter(|&&c| c).count();
    }

    /// Create network and add all routers.
    fn create_network_with_routers(&mut self) -> Result<ScionNetwork, GraphToolError> {
        let mut net: ScionNetwork = Network::default();

        // Add all routers
        for node in &self.graph.nodes {
            let isd_as = IsdAs::new(node.isd, node.asn);
            let is_core = self.as_is_core.get(&isd_as).copied().unwrap_or(false);

            // Create router name
            let name = format!("as{}_br{}", node.asn, node.id);

            // Add router
            let router_id = net.add_router(name, node.asn);

            // Enable SCION
            net.enable_scion_router(router_id, isd_as, is_core)
                .map_err(|e| GraphToolError::Network {
                    message: format!(
                        "Failed to enable SCION for router {} (AS {}): {}",
                        node.id, isd_as, e
                    ),
                })?;

            // Track mappings
            self.node_to_router.insert(node.id, router_id);
            self.as_to_routers
                .entry(isd_as)
                .or_default()
                .push(router_id);
        }

        self.stats.router_count = self.graph.nodes.len();
        Ok(net)
    }

    /// Add physical links in batch for efficiency.
    fn add_physical_links(&mut self, net: &mut ScionNetwork) -> Result<(), GraphToolError> {
        let links: Vec<(RouterId, RouterId)> = self
            .graph
            .edges
            .iter()
            .filter_map(|edge| {
                let src = self.node_to_router.get(&edge.source)?;
                let dst = self.node_to_router.get(&edge.target)?;
                Some((*src, *dst))
            })
            .collect();

        self.stats.link_count = links.len();

        net.add_links_from(links).map_err(|e| GraphToolError::Network {
            message: format!("Failed to add physical links: {}", e),
        })?;

        Ok(())
    }

    /// Add SCION link metadata for inter-AS edges.
    fn add_scion_links(&mut self, net: &mut ScionNetwork) -> Result<(), GraphToolError> {
        for edge in &self.graph.edges {
            if !edge.inter_as {
                self.stats.intra_as_link_count += 1;
                continue;
            }

            let src_router = match self.node_to_router.get(&edge.source) {
                Some(r) => *r,
                None => continue,
            };
            let dst_router = match self.node_to_router.get(&edge.target) {
                Some(r) => *r,
                None => continue,
            };

            // Determine link type based on AS core status
            let src_node = &self.graph.nodes[edge.source as usize];
            let dst_node = &self.graph.nodes[edge.target as usize];

            let src_isd_as = IsdAs::new(src_node.isd, src_node.asn);
            let dst_isd_as = IsdAs::new(dst_node.isd, dst_node.asn);

            let src_is_core = self.as_is_core.get(&src_isd_as).copied().unwrap_or(false);
            let dst_is_core = self.as_is_core.get(&dst_isd_as).copied().unwrap_or(false);

            let link_type = determine_link_type(
                src_isd_as,
                dst_isd_as,
                src_is_core,
                dst_is_core,
            );

            net.add_scion_link(src_router, dst_router, link_type)
                .map_err(|e| GraphToolError::Network {
                    message: format!(
                        "Failed to add SCION link {} -> {}: {}",
                        src_isd_as, dst_isd_as, e
                    ),
                })?;

            self.stats.scion_link_count += 1;
        }

        Ok(())
    }

    /// Get all unique ASes in the graph.
    pub fn get_all_ases(&self) -> Vec<IsdAs> {
        self.as_to_routers.keys().copied().collect()
    }

    /// Get all core ASes.
    pub fn get_core_ases(&self) -> Vec<IsdAs> {
        self.as_is_core
            .iter()
            .filter_map(|(isd_as, &is_core)| if is_core { Some(*isd_as) } else { None })
            .collect()
    }
}

/// Determine SCION link type based on AS properties.
///
/// Rules:
/// - Core-to-Core (same ISD): Core
/// - Core-to-Core (different ISD): Core
/// - Core-to-NonCore: Child (from core's perspective)
/// - NonCore-to-NonCore (same ISD): Could be Peer or sibling, default to Peer
fn determine_link_type(
    src_isd_as: IsdAs,
    dst_isd_as: IsdAs,
    src_is_core: bool,
    dst_is_core: bool,
) -> ScionLinkType {
    match (src_is_core, dst_is_core) {
        // Both core: Core link
        (true, true) => ScionLinkType::Core,

        // Core to non-core: Child link (from core's perspective)
        // The non-core sees this as Parent
        (true, false) => ScionLinkType::Child,

        // Non-core to core: Parent link from non-core's perspective
        // add_scion_link(A, B, Parent) sets A=Parent, B=Child
        // So non-core sees core as Parent, core sees non-core as Child
        (false, true) => ScionLinkType::Parent,

        // Non-core to non-core in same ISD: could be peering
        (false, false) if src_isd_as.isd == dst_isd_as.isd => ScionLinkType::Peer,

        // Non-core to non-core in different ISDs: unusual, treat as peer
        (false, false) => ScionLinkType::Peer,
    }
}

/// Helper to build and run beaconing in one call.
pub fn build_and_run_beaconing(
    graph: GtGraph,
) -> Result<(ScionNetwork, BuildStats), GraphToolError> {
    let builder = ScionNetworkBuilder::new(graph);

    // Collect AS info before consuming builder
    let as_info: BTreeMap<IsdAs, bool> = builder.graph.nodes.iter().fold(
        BTreeMap::new(),
        |mut acc, node| {
            let isd_as = IsdAs::new(node.isd, node.asn);
            let entry = acc.entry(isd_as).or_insert(false);
            if node.core {
                *entry = true;
            }
            acc
        },
    );

    let (mut net, stats) = builder.build()?;

    // Run beaconing
    let core_ases: Vec<IsdAs> = as_info
        .iter()
        .filter_map(|(isd_as, &is_core)| if is_core { Some(*isd_as) } else { None })
        .collect();
    let all_ases: Vec<IsdAs> = as_info.keys().copied().collect();

    run_beaconing(&mut net, &core_ases, &all_ases)?;

    Ok((net, stats))
}

/// Run SCION beaconing phases on a network.
pub fn run_beaconing(
    net: &mut ScionNetwork,
    core_ases: &[IsdAs],
    all_ases: &[IsdAs],
) -> Result<(), GraphToolError> {
    run_beaconing_with_progress(net, core_ases, all_ases, false)
}

/// Run SCION beaconing phases on a network with optional progress reporting.
pub fn run_beaconing_with_progress(
    net: &mut ScionNetwork,
    core_ases: &[IsdAs],
    all_ases: &[IsdAs],
    verbose: bool,
) -> Result<(), GraphToolError> {
    // Calculate required rounds based on number of ISDs
    let unique_isds: BTreeSet<u16> = all_ases.iter().map(|a| a.isd.0).collect();
    let num_isds = unique_isds.len();

    // Phase 1: Core beaconing
    // Need enough rounds for PCBs to propagate through the core mesh
    // For dense core networks, need diameter rounds (typically small)
    // For sparse cores, may need more rounds
    let core_rounds = std::cmp::min(
        std::cmp::max(num_isds * 2, (core_ases.len() as f64).sqrt().ceil() as usize + 2),
        15,  // Cap at 15 rounds max for dense meshes
    );

    if verbose {
        eprintln!("  Phase 1: Core beaconing ({} rounds for {} cores)...", core_rounds, core_ases.len());
    }

    for round in 0..core_rounds {
        if verbose && round % 2 == 0 {
            eprintln!("    Round {}/{}...", round + 1, core_rounds);
        }
        net.propagate_core_batch(core_ases)
            .map_err(|e| GraphToolError::Network {
                message: format!("Core beaconing failed: {}", e),
            })?;
    }

    // Phase 2: Intra-ISD beaconing (propagate down the hierarchy)
    if verbose {
        eprintln!("  Phase 2: Intra-ISD beaconing...");
    }

    // For hierarchical networks: propagate level by level until no new ASes
    // For flat/mesh networks: limit iterations to prevent cycling
    let mut current_level: Vec<IsdAs> = core_ases.to_vec();
    let mut level = 0;
    let mut seen_ases: BTreeSet<IsdAs> = core_ases.iter().copied().collect();
    let max_levels = std::cmp::min(all_ases.len(), 20); // Cap based on network depth

    while !current_level.is_empty() && level < max_levels {
        if verbose {
            eprintln!("    Level {}: {} ASes...", level, current_level.len());
        }
        let next_level = net
            .propagate_intra_isd_batch(&current_level)
            .map_err(|e| GraphToolError::Network {
                message: format!("Intra-ISD beaconing failed: {}", e),
            })?;

        // Only include ASes we haven't seen before to prevent cycling
        let new_ases: Vec<IsdAs> = next_level
            .into_iter()
            .filter(|a| seen_ases.insert(*a))
            .collect();

        current_level = new_ases;
        level += 1;
    }

    if verbose && level >= max_levels && !current_level.is_empty() {
        eprintln!("    Reached max depth {}, {} ASes still pending", max_levels, current_level.len());
    }

    // Phase 3: Segment registration
    if verbose {
        eprintln!("  Phase 3: Segment registration ({} ASes)...", all_ases.len());
    }

    net.register_segments_batch(all_ases)
        .map_err(|e| GraphToolError::Network {
            message: format!("Segment registration failed: {}", e),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_graph_from_str;

    #[test]
    fn test_determine_link_type() {
        let core1 = IsdAs::new(1, 100);
        let core2 = IsdAs::new(1, 101);
        let leaf = IsdAs::new(1, 200);
        let cross_isd_core = IsdAs::new(2, 100);

        // Core to core same ISD
        assert!(matches!(
            determine_link_type(core1, core2, true, true),
            ScionLinkType::Core
        ));

        // Core to core different ISD
        assert!(matches!(
            determine_link_type(core1, cross_isd_core, true, true),
            ScionLinkType::Core
        ));

        // Core to leaf
        assert!(matches!(
            determine_link_type(core1, leaf, true, false),
            ScionLinkType::Child
        ));

        // Leaf to leaf (peering)
        let leaf2 = IsdAs::new(1, 201);
        assert!(matches!(
            determine_link_type(leaf, leaf2, false, false),
            ScionLinkType::Peer
        ));
    }

    #[test]
    fn test_build_simple_network() {
        let json = r#"{
            "nodes": [
                {"id": 0, "asn": 100, "isd": 1, "border_router": true, "core": true, "interfaces": []},
                {"id": 1, "asn": 101, "isd": 1, "border_router": true, "core": true, "interfaces": []},
                {"id": 2, "asn": 200, "isd": 1, "border_router": true, "core": false, "interfaces": []}
            ],
            "edges": [
                {"source": 0, "target": 1, "inter_as": true, "rtt_sec": 0.001, "distance_km": 100.0, "skipped_hops": 0},
                {"source": 0, "target": 2, "inter_as": true, "rtt_sec": 0.002, "distance_km": 200.0, "skipped_hops": 0}
            ]
        }"#;

        let graph = load_graph_from_str(json).unwrap();
        let builder = ScionNetworkBuilder::new(graph);
        let (net, stats) = builder.build().unwrap();

        assert_eq!(stats.router_count, 3);
        assert_eq!(stats.as_count, 3);
        assert_eq!(stats.core_as_count, 2);
        assert_eq!(stats.link_count, 2);
        assert_eq!(stats.scion_link_count, 2);
    }
}
