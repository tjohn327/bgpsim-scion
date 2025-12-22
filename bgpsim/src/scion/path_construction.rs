// Path Construction Algorithms
//
// This module implements algorithms for combining path segments to create
// end-to-end paths. It handles all standard path types:
// - Intra-AS (empty path)
// - Intra-ISD (up + down)
// - Inter-ISD (up + core + down)
// - Core-to-core (core segment only)
// - Core-to-non-core (down segment only)
// - Non-core-to-core (up segment only)

use super::path_db::{PathDatabase, PathSegment};
use super::path_query::{PathQuery, PathQueryResult, PathSegmentRef, PeeringShortcut, ScionPath};
use std::collections::HashSet;

/// Construct paths based on a path query (without peering shortcuts)
///
/// This is the main entry point for path construction. It determines the
/// appropriate construction strategy based on source and destination properties.
pub fn construct_paths(
    query: &PathQuery,
    db: &PathDatabase,
    src_is_core: bool,
    dst_is_core: bool,
) -> PathQueryResult {
    let src = query.src;
    let dst = query.dst;

    // Case 1: Same AS
    if src == dst {
        let path = ScionPath::intra_as(src);
        return PathQueryResult::success(query.clone(), vec![path]);
    }

    // Case 2: Core to core
    if src_is_core && dst_is_core {
        return construct_core_to_core_paths(query, db);
    }

    // Case 3: Core to non-core
    if src_is_core && !dst_is_core {
        return construct_core_to_noncore_paths(query, db);
    }

    // Case 4: Non-core to core
    if !src_is_core && dst_is_core {
        return construct_noncore_to_core_paths(query, db);
    }

    // Case 5: Non-core to non-core
    let same_isd = src.isd == dst.isd;
    if same_isd {
        construct_intra_isd_paths(query, db)
    } else {
        construct_inter_isd_paths(query, db)
    }
}

/// Construct core-to-core paths (single core segment)
fn construct_core_to_core_paths(query: &PathQuery, db: &PathDatabase) -> PathQueryResult {
    let core_segments = db.get_core(query.src, query.dst);

    let paths: Vec<ScionPath> = core_segments
        .into_iter()
        .map(|seg| {
            ScionPath::new(
                query.src,
                query.dst,
                vec![PathSegmentRef::Core(seg.clone())],
                None,
            )
        })
        .take(query.max_paths)
        .collect();

    // If no direct core segment, the ASes may not be connected
    if paths.is_empty() {
        return PathQueryResult::error(
            query.clone(),
            format!("No core segment from {} to {}", query.src, query.dst),
        );
    }

    PathQueryResult::success(query.clone(), paths)
}

/// Construct core-to-non-core paths (single down segment)
fn construct_core_to_noncore_paths(query: &PathQuery, db: &PathDatabase) -> PathQueryResult {
    let down_segments = db.get_down_from_to(query.src, query.dst);

    let paths: Vec<ScionPath> = down_segments
        .into_iter()
        .map(|seg| {
            ScionPath::new(
                query.src,
                query.dst,
                vec![PathSegmentRef::Down(seg.clone())],
                None,
            )
        })
        .take(query.max_paths)
        .collect();

    if paths.is_empty() {
        return PathQueryResult::error(
            query.clone(),
            format!("No down segment from {} to {}", query.src, query.dst),
        );
    }

    PathQueryResult::success(query.clone(), paths)
}

/// Construct non-core-to-core paths (single up segment)
fn construct_noncore_to_core_paths(query: &PathQuery, db: &PathDatabase) -> PathQueryResult {
    let up_segments = db.get_up_from(query.src);

    // Filter to segments that reach the destination core
    let paths: Vec<ScionPath> = up_segments
        .into_iter()
        .filter(|seg| seg.src() == Some(query.dst))
        .map(|seg| {
            ScionPath::new(
                query.src,
                query.dst,
                vec![PathSegmentRef::Up(seg.clone())],
                None,
            )
        })
        .take(query.max_paths)
        .collect();

    if paths.is_empty() {
        return PathQueryResult::error(
            query.clone(),
            format!("No up segment from {} to {}", query.src, query.dst),
        );
    }

    PathQueryResult::success(query.clone(), paths)
}

/// Construct intra-ISD paths (up + down)
///
/// For non-core ASes in the same ISD, paths are constructed by combining:
/// 1. Up segment from source to a core AS
/// 2. Down segment from that core AS to destination
fn construct_intra_isd_paths(query: &PathQuery, db: &PathDatabase) -> PathQueryResult {
    let mut paths = Vec::new();

    // Get all up segments from source
    let up_segments = db.get_up_from(query.src);

    if up_segments.is_empty() {
        return PathQueryResult::error(
            query.clone(),
            format!("No up segments from {}", query.src),
        );
    }

    // For each up segment, try to find matching down segment
    for up_seg in up_segments {
        // Core AS is the source of the up segment
        let core_as = match up_seg.src() {
            Some(as_id) => as_id,
            None => continue,
        };

        // Get down segments from this core to destination
        let down_segments = db.get_down_from_to(core_as, query.dst);

        for down_seg in down_segments {
            let path = ScionPath::new(
                query.src,
                query.dst,
                vec![
                    PathSegmentRef::Up(up_seg.clone()),
                    PathSegmentRef::Down(down_seg.clone()),
                ],
                None,
            );

            paths.push(path);

            // Check if we've reached max_paths
            if paths.len() >= query.max_paths {
                return PathQueryResult::success(query.clone(), paths);
            }
        }
    }

    if paths.is_empty() {
        return PathQueryResult::error(
            query.clone(),
            format!(
                "No complete path from {} to {} in same ISD",
                query.src, query.dst
            ),
        );
    }

    PathQueryResult::success(query.clone(), paths)
}

/// Construct inter-ISD paths (up + core + down)
///
/// For non-core ASes in different ISDs, paths are constructed by combining:
/// 1. Up segment from source to source's core AS
/// 2. Core segment from source's core to destination's core
/// 3. Down segment from destination's core to destination
fn construct_inter_isd_paths(query: &PathQuery, db: &PathDatabase) -> PathQueryResult {
    let mut paths = Vec::new();

    // Get all up segments from source
    let up_segments = db.get_up_from(query.src);

    if up_segments.is_empty() {
        return PathQueryResult::error(
            query.clone(),
            format!("No up segments from {}", query.src),
        );
    }

    // Track which (src_core, dst_core) pairs we've seen to avoid duplicates
    let mut seen_pairs = HashSet::new();

    for up_seg in up_segments {
        // Source core AS
        let src_core = match up_seg.src() {
            Some(as_id) => as_id,
            None => continue,
        };

        // Get core segments to destination ISD
        let dst_isd = query.dst.isd.0;
        let core_segments = db.get_core_to_isd(src_core, dst_isd);

        for core_seg in core_segments {
            // Destination core AS
            let dst_core = match core_seg.dst() {
                Some(as_id) => as_id,
                None => continue,
            };

            // Skip if we've already found paths through this core pair
            if !seen_pairs.insert((src_core, dst_core)) {
                continue;
            }

            // Get down segments from destination core to destination
            let down_segments = db.get_down_from_to(dst_core, query.dst);

            for down_seg in down_segments {
                let path = ScionPath::new(
                    query.src,
                    query.dst,
                    vec![
                        PathSegmentRef::Up(up_seg.clone()),
                        PathSegmentRef::Core(core_seg.clone()),
                        PathSegmentRef::Down(down_seg.clone()),
                    ],
                    None,
                );

                paths.push(path);

                // Check if we've reached max_paths
                if paths.len() >= query.max_paths {
                    return PathQueryResult::success(query.clone(), paths);
                }
            }
        }
    }

    if paths.is_empty() {
        return PathQueryResult::error(
            query.clone(),
            format!(
                "No complete path from {} to {} across ISDs",
                query.src, query.dst
            ),
        );
    }

    PathQueryResult::success(query.clone(), paths)
}

/// Find peering shortcuts between up and down segments
///
/// This function looks for peering links advertised in AS entries that
/// can create shortcuts between up and down segments.
fn find_peering_shortcuts(up_seg: &PathSegment, down_seg: &PathSegment) -> Vec<PeeringShortcut> {
    let mut shortcuts = Vec::new();

    // Iterate through AS entries in up segment
    for up_entry in &up_seg.pcb.as_entries {
        // Check all peer entries in this AS entry
        for peer in &up_entry.peer_entries {
            let peer_as = peer.peer_isd_as;

            // Check if peer AS appears in down segment
            for down_entry in &down_seg.pcb.as_entries {
                if down_entry.isd_as == peer_as {
                    // Found a potential shortcut
                    let shortcut = PeeringShortcut::new(
                        up_entry.isd_as,
                        peer_as,
                        peer.hop_field.ingress,
                        peer.peer_interface,
                        peer.clone(),
                    );
                    shortcuts.push(shortcut);
                }
            }
        }
    }

    shortcuts
}

/// Construct paths with peering shortcuts
///
/// This extends the basic path construction to include peering shortcuts.
/// It first finds all standard paths, then looks for peering shortcuts
/// between up and down segments.
pub fn construct_paths_with_peering(
    query: &PathQuery,
    db: &PathDatabase,
    src_is_core: bool,
    dst_is_core: bool,
) -> PathQueryResult {
    // First, get all standard paths
    let result = construct_paths(query, db, src_is_core, dst_is_core);

    // If not allowing peering, return standard paths
    if !query.allow_peering {
        return result;
    }

    // If no standard paths or query parameters don't support peering, return as-is
    if result.paths.is_empty() || src_is_core || dst_is_core || query.src == query.dst {
        return result;
    }

    let mut all_paths = result.paths;

    // Now look for peering shortcuts
    // Peering can shortcut between up and down segments
    let up_segments = db.get_up_from(query.src);
    let down_segments = db.get_down_to(query.dst);

    for up_seg in up_segments {
        for down_seg in &down_segments {
            // Find all possible peering shortcuts
            let shortcuts = find_peering_shortcuts(&up_seg, down_seg);

            for shortcut in shortcuts {
                // Create path using the shortcut
                let path = ScionPath::new(
                    query.src,
                    query.dst,
                    vec![
                        PathSegmentRef::Up(up_seg.clone()),
                        PathSegmentRef::Down(down_seg.clone()),
                    ],
                    Some(shortcut),
                );

                all_paths.push(path);

                // Check if we've reached max_paths
                if all_paths.len() >= query.max_paths {
                    return PathQueryResult::success(query.clone(), all_paths);
                }
            }
        }
    }

    PathQueryResult::success(query.clone(), all_paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scion::pcb::{AsEntry, HopEntry, Pcb};
    use crate::scion::path_db::{PathSegment, SegmentType};
    use crate::scion::types::{InterfaceId, IsdAs};
    use std::sync::Arc;

    fn create_up_segment(noncore: IsdAs, core: IsdAs) -> Arc<PathSegment> {
        let mut pcb = Pcb::new(core);
        pcb.extend(AsEntry::new(
            core,
            Some(noncore),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        pcb.extend(AsEntry::new(
            noncore,
            None,
            HopEntry::new(InterfaceId::new(1), None),
        ));
        Arc::new(PathSegment::new(SegmentType::Up, pcb))
    }

    fn create_down_segment(core: IsdAs, noncore: IsdAs) -> Arc<PathSegment> {
        let mut pcb = Pcb::new(core);
        pcb.extend(AsEntry::new(
            core,
            Some(noncore),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        pcb.extend(AsEntry::new(
            noncore,
            None,
            HopEntry::new(InterfaceId::new(1), None),
        ));
        Arc::new(PathSegment::new(SegmentType::Down, pcb))
    }

    fn create_core_segment(src_core: IsdAs, dst_core: IsdAs) -> Arc<PathSegment> {
        let mut pcb = Pcb::new(src_core);
        pcb.extend(AsEntry::new(
            src_core,
            Some(dst_core),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        pcb.extend(AsEntry::new(
            dst_core,
            None,
            HopEntry::new(InterfaceId::new(1), None),
        ));
        Arc::new(PathSegment::new(SegmentType::Core, pcb))
    }

    #[test]
    fn test_intra_as_path() {
        let as1 = IsdAs::new(1, 100);
        let query = PathQuery::new(as1, as1);
        let db = PathDatabase::new();

        let result = construct_paths(&query, &db, false, false);

        assert!(result.is_success());
        assert_eq!(result.paths.len(), 1);
        assert!(result.paths[0].is_intra_as());
    }

    #[test]
    fn test_core_to_core_path() {
        let core1 = IsdAs::new(1, 100);
        let core2 = IsdAs::new(1, 200);
        let query = PathQuery::new(core1, core2);

        let mut db = PathDatabase::new();
        db.add_segment(create_core_segment(core1, core2));

        let result = construct_paths(&query, &db, true, true);

        assert!(result.is_success());
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].segment_count(), 1);
    }

    #[test]
    fn test_core_to_noncore_path() {
        let core = IsdAs::new(1, 100);
        let noncore = IsdAs::new(1, 200);
        let query = PathQuery::new(core, noncore);

        let mut db = PathDatabase::new();
        db.add_segment(create_down_segment(core, noncore));

        let result = construct_paths(&query, &db, true, false);

        assert!(result.is_success());
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].segment_count(), 1);
    }

    #[test]
    fn test_noncore_to_core_path() {
        let core = IsdAs::new(1, 100);
        let noncore = IsdAs::new(1, 200);
        let query = PathQuery::new(noncore, core);

        let mut db = PathDatabase::new();
        db.add_segment(create_up_segment(noncore, core));

        let result = construct_paths(&query, &db, false, true);

        assert!(result.is_success());
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].segment_count(), 1);
    }

    #[test]
    fn test_intra_isd_path() {
        let core = IsdAs::new(1, 100);
        let as1 = IsdAs::new(1, 200);
        let as2 = IsdAs::new(1, 300);
        let query = PathQuery::new(as1, as2);

        let mut db = PathDatabase::new();
        db.add_segment(create_up_segment(as1, core));
        db.add_segment(create_down_segment(core, as2));

        let result = construct_paths(&query, &db, false, false);

        assert!(result.is_success());
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].segment_count(), 2); // up + down
    }

    #[test]
    fn test_inter_isd_path() {
        let core1 = IsdAs::new(1, 100);
        let core2 = IsdAs::new(2, 100);
        let as1 = IsdAs::new(1, 200);
        let as2 = IsdAs::new(2, 200);
        let query = PathQuery::new(as1, as2);

        let mut db = PathDatabase::new();
        db.add_segment(create_up_segment(as1, core1));
        db.add_segment(create_core_segment(core1, core2));
        db.add_segment(create_down_segment(core2, as2));

        let result = construct_paths(&query, &db, false, false);

        assert!(result.is_success());
        assert_eq!(result.paths.len(), 1);
        assert_eq!(result.paths[0].segment_count(), 3); // up + core + down
    }

    #[test]
    fn test_no_path_available() {
        let as1 = IsdAs::new(1, 200);
        let as2 = IsdAs::new(1, 300);
        let query = PathQuery::new(as1, as2);

        let db = PathDatabase::new(); // Empty database

        let result = construct_paths(&query, &db, false, false);

        assert!(result.is_error());
        assert!(result.error.unwrap().contains("No up segments"));
    }

    #[test]
    fn test_multiple_paths() {
        let core = IsdAs::new(1, 100);
        let as1 = IsdAs::new(1, 200);
        let as2 = IsdAs::new(1, 300);
        let query = PathQuery::new(as1, as2).with_max_paths(5);

        let mut db = PathDatabase::new();

        // Add multiple up and down segments
        db.add_segment(create_up_segment(as1, core));
        db.add_segment(create_down_segment(core, as2));

        // Add a second down segment
        let mut pcb2 = Pcb::new(core);
        pcb2.extend(AsEntry::new(
            core,
            Some(as2),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(2))),
        ));
        pcb2.extend(AsEntry::new(
            as2,
            None,
            HopEntry::new(InterfaceId::new(2), None),
        ));
        db.add_segment(Arc::new(PathSegment::new(SegmentType::Down, pcb2)));

        let result = construct_paths(&query, &db, false, false);

        assert!(result.is_success());
        assert_eq!(result.paths.len(), 2); // 1 up * 2 down = 2 paths
    }

    #[test]
    fn test_peering_shortcut_detection() {
        use crate::scion::pcb::PeerEntry;

        let core1 = IsdAs::new(1, 100);
        let core2 = IsdAs::new(1, 200);
        let as1 = IsdAs::new(1, 300);
        let as2 = IsdAs::new(1, 400);

        // Create up segment with peering information
        let mut up_pcb = Pcb::new(core1);
        up_pcb.extend(AsEntry::new(
            core1,
            Some(as1),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));

        // Add AS1 with peering to AS2
        let mut as1_entry = AsEntry::new(
            as1,
            None,
            HopEntry::new(InterfaceId::new(1), None),
        );
        let peer = PeerEntry::new(
            as2,
            InterfaceId::new(10),
            HopEntry::new(InterfaceId::new(5), Some(InterfaceId::new(10))),
        );
        as1_entry.add_peer_entry(peer);
        up_pcb.extend(as1_entry);

        let up_seg = Arc::new(PathSegment::new(SegmentType::Up, up_pcb));

        // Create down segment containing AS2
        let mut down_pcb = Pcb::new(core2);
        down_pcb.extend(AsEntry::new(
            core2,
            Some(as2),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(2))),
        ));
        down_pcb.extend(AsEntry::new(
            as2,
            None,
            HopEntry::new(InterfaceId::new(2), None),
        ));
        let down_seg = Arc::new(PathSegment::new(SegmentType::Down, down_pcb));

        // Find peering shortcuts
        let shortcuts = find_peering_shortcuts(&up_seg, &down_seg);

        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0].up_side_as, as1);
        assert_eq!(shortcuts[0].down_side_as, as2);
    }

    #[test]
    fn test_construct_path_with_peering() {
        use crate::scion::pcb::PeerEntry;

        let core = IsdAs::new(1, 100);
        let as1 = IsdAs::new(1, 200);
        let as2 = IsdAs::new(1, 300);

        let mut db = PathDatabase::new();

        // Create up segment with peering
        let mut up_pcb = Pcb::new(core);
        up_pcb.extend(AsEntry::new(
            core,
            Some(as1),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));

        let mut as1_entry = AsEntry::new(
            as1,
            None,
            HopEntry::new(InterfaceId::new(1), None),
        );
        let peer = PeerEntry::new(
            as2,
            InterfaceId::new(10),
            HopEntry::new(InterfaceId::new(5), Some(InterfaceId::new(10))),
        );
        as1_entry.add_peer_entry(peer);
        up_pcb.extend(as1_entry);

        db.add_segment(Arc::new(PathSegment::new(SegmentType::Up, up_pcb)));

        // Create down segment
        db.add_segment(create_down_segment(core, as2));

        // Query with peering enabled
        let query = PathQuery::new(as1, as2).with_peering(true);
        let result = construct_paths_with_peering(&query, &db, false, false);

        assert!(result.is_success());
        assert!(result.paths.len() >= 2); // At least standard path + peering path

        // Find the peering path
        let peering_path = result.paths.iter().find(|p| p.uses_peering());
        assert!(peering_path.is_some());
    }

    #[test]
    fn test_peering_disabled() {
        use crate::scion::pcb::PeerEntry;

        let core = IsdAs::new(1, 100);
        let as1 = IsdAs::new(1, 200);
        let as2 = IsdAs::new(1, 300);

        let mut db = PathDatabase::new();

        // Create segments with peering
        let mut up_pcb = Pcb::new(core);
        up_pcb.extend(AsEntry::new(
            core,
            Some(as1),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));

        let mut as1_entry = AsEntry::new(
            as1,
            None,
            HopEntry::new(InterfaceId::new(1), None),
        );
        as1_entry.add_peer_entry(PeerEntry::new(
            as2,
            InterfaceId::new(10),
            HopEntry::new(InterfaceId::new(5), Some(InterfaceId::new(10))),
        ));
        up_pcb.extend(as1_entry);

        db.add_segment(Arc::new(PathSegment::new(SegmentType::Up, up_pcb)));
        db.add_segment(create_down_segment(core, as2));

        // Query with peering disabled
        let query = PathQuery::new(as1, as2).with_peering(false);
        let result = construct_paths_with_peering(&query, &db, false, false);

        assert!(result.is_success());
        // Should only have standard paths, no peering paths
        assert!(result.paths.iter().all(|p| !p.uses_peering()));
    }

    #[test]
    fn test_no_peering_in_core_paths() {
        let core1 = IsdAs::new(1, 100);
        let core2 = IsdAs::new(1, 200);

        let mut db = PathDatabase::new();
        db.add_segment(create_core_segment(core1, core2));

        // Peering is not applicable for core-to-core
        let query = PathQuery::new(core1, core2).with_peering(true);
        let result = construct_paths_with_peering(&query, &db, true, true);

        assert!(result.is_success());
        assert_eq!(result.paths.len(), 1);
        assert!(!result.paths[0].uses_peering());
    }
}
