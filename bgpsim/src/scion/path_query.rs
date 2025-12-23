// Path Query and Construction
//
// This module implements SCION path lookup and construction, combining
// up, core, and down segments to create end-to-end paths. It also supports
// peering shortcuts that bypass standard hierarchical routing.

use super::path_db::{PathSegment, SegmentType};
use super::pcb::PeerEntry;
use super::types::{IsdAs, InterfaceId};
use std::sync::Arc;

/// Hop information for forwarding
///
/// Represents a single hop in a SCION path with the AS identifier
/// and the ingress/egress interface IDs needed for forwarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HopInfo {
    /// The AS at this hop
    pub isd_as: IsdAs,

    /// Ingress interface ID (where packet enters this AS)
    /// None for the source AS
    pub ingress: Option<InterfaceId>,

    /// Egress interface ID (where packet exits this AS)
    /// None for the destination AS
    pub egress: Option<InterfaceId>,
}

impl HopInfo {
    /// Create a new hop info
    pub fn new(isd_as: IsdAs, ingress: Option<InterfaceId>, egress: Option<InterfaceId>) -> Self {
        Self { isd_as, ingress, egress }
    }
}

/// Query for paths between two ASes
///
/// This structure represents a request to find paths from a source AS
/// to a destination AS.
#[derive(Debug, Clone)]
pub struct PathQuery {
    /// Source ISD-AS
    pub src: IsdAs,

    /// Destination ISD-AS
    pub dst: IsdAs,

    /// Maximum number of paths to return
    pub max_paths: usize,

    /// Whether to include peering shortcuts
    pub allow_peering: bool,
}

impl PathQuery {
    /// Create a new path query
    pub fn new(src: IsdAs, dst: IsdAs) -> Self {
        Self {
            src,
            dst,
            max_paths: 10,
            allow_peering: true,
        }
    }

    /// Set maximum number of paths
    pub fn with_max_paths(mut self, max_paths: usize) -> Self {
        self.max_paths = max_paths;
        self
    }

    /// Set whether to allow peering shortcuts
    pub fn with_peering(mut self, allow_peering: bool) -> Self {
        self.allow_peering = allow_peering;
        self
    }
}

/// Reference to a segment in a path
///
/// This enum wraps Arc<PathSegment> references with explicit type information
/// to make path construction more type-safe.
#[derive(Debug, Clone)]
pub enum PathSegmentRef {
    /// Up segment (non-core to core)
    Up(Arc<PathSegment>),

    /// Core segment (core to core)
    Core(Arc<PathSegment>),

    /// Down segment (core to non-core)
    Down(Arc<PathSegment>),
}

impl PathSegmentRef {
    /// Get the underlying segment
    pub fn segment(&self) -> &Arc<PathSegment> {
        match self {
            Self::Up(s) => s,
            Self::Core(s) => s,
            Self::Down(s) => s,
        }
    }

    /// Get the segment type
    pub fn segment_type(&self) -> SegmentType {
        match self {
            Self::Up(_) => SegmentType::Up,
            Self::Core(_) => SegmentType::Core,
            Self::Down(_) => SegmentType::Down,
        }
    }

    /// Get the length of this segment
    pub fn len(&self) -> usize {
        self.segment().len()
    }

    /// Check if the segment is empty
    pub fn is_empty(&self) -> bool {
        self.segment().is_empty()
    }
}

/// Peering shortcut information
///
/// A peering shortcut allows bypassing part of the standard up-core-down
/// path structure by using a direct peering link between two ASes.
#[derive(Debug, Clone)]
pub struct PeeringShortcut {
    /// AS on the up-segment side
    pub up_side_as: IsdAs,

    /// AS on the down-segment side
    pub down_side_as: IsdAs,

    /// Interface on up-side AS
    pub up_interface: InterfaceId,

    /// Interface on down-side AS
    pub down_interface: InterfaceId,

    /// Peer entry providing the shortcut
    pub peer_entry: PeerEntry,
}

impl PeeringShortcut {
    /// Create a new peering shortcut
    pub fn new(
        up_side_as: IsdAs,
        down_side_as: IsdAs,
        up_interface: InterfaceId,
        down_interface: InterfaceId,
        peer_entry: PeerEntry,
    ) -> Self {
        Self {
            up_side_as,
            down_side_as,
            up_interface,
            down_interface,
            peer_entry,
        }
    }

    /// Get the AS pair connected by this peering link
    pub fn as_pair(&self) -> (IsdAs, IsdAs) {
        (self.up_side_as, self.down_side_as)
    }

    /// Get the interface pair for this peering link
    pub fn interface_pair(&self) -> (InterfaceId, InterfaceId) {
        (self.up_interface, self.down_interface)
    }
}

/// A complete end-to-end path
///
/// A SCION path consists of one or more path segments combined to create
/// an end-to-end route. Optionally, a peering shortcut can bypass part
/// of the standard hierarchical routing.
#[derive(Debug, Clone)]
pub struct ScionPath {
    /// Source AS
    pub src: IsdAs,

    /// Destination AS
    pub dst: IsdAs,

    /// Segments composing the path
    pub segments: Vec<PathSegmentRef>,

    /// Optional peering shortcut information
    pub peering_shortcut: Option<PeeringShortcut>,

    /// Total path length (number of AS hops)
    pub total_length: usize,
}

impl ScionPath {
    /// Create a new SCION path
    pub fn new(
        src: IsdAs,
        dst: IsdAs,
        segments: Vec<PathSegmentRef>,
        peering_shortcut: Option<PeeringShortcut>,
    ) -> Self {
        let total_length = if let Some(ref shortcut) = peering_shortcut {
            Self::calculate_shortcut_length(&segments, shortcut)
        } else {
            segments.iter().map(|s| s.len()).sum()
        };

        Self {
            src,
            dst,
            segments,
            peering_shortcut,
            total_length,
        }
    }

    /// Create an intra-AS path (empty path)
    pub fn intra_as(isd_as: IsdAs) -> Self {
        Self {
            src: isd_as,
            dst: isd_as,
            segments: Vec::new(),
            peering_shortcut: None,
            total_length: 0,
        }
    }

    /// Check if this path is intra-AS (no segments)
    pub fn is_intra_as(&self) -> bool {
        self.src == self.dst && self.segments.is_empty()
    }

    /// Check if this path uses a peering shortcut
    pub fn uses_peering(&self) -> bool {
        self.peering_shortcut.is_some()
    }

    /// Get the number of segments in this path
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Get the AS-level path (all ASes traversed)
    pub fn as_path(&self) -> Vec<IsdAs> {
        if self.is_intra_as() {
            return vec![self.src];
        }

        let mut path = Vec::new();

        for (i, seg_ref) in self.segments.iter().enumerate() {
            let seg = seg_ref.segment();
            let mut as_path = seg.as_path();

            // UP segments are traversed in reverse of their PCB direction
            if matches!(seg_ref, PathSegmentRef::Up(_)) {
                as_path.reverse();
            }

            if i == 0 {
                // First segment: include all ASes
                path.extend(as_path);
            } else {
                // Subsequent segments: skip first AS (already included from previous segment)
                path.extend(as_path.into_iter().skip(1));
            }
        }

        path
    }

    /// Get the full hop fields with ingress/egress interfaces
    ///
    /// Returns a vector of HopInfo structs, one per AS hop, containing
    /// the AS identifier and the ingress/egress interface IDs for forwarding.
    ///
    /// For UP segments (traversed in reverse), ingress/egress are swapped
    /// to reflect the actual packet direction.
    pub fn hop_fields(&self) -> Vec<HopInfo> {
        if self.is_intra_as() {
            return vec![HopInfo::new(self.src, None, None)];
        }

        let mut hops = Vec::new();

        for (i, seg_ref) in self.segments.iter().enumerate() {
            let seg = seg_ref.segment();
            let is_up = matches!(seg_ref, PathSegmentRef::Up(_));

            // Get AS entries from the segment
            let entries = &seg.pcb.as_entries;

            // For UP segments, we traverse in reverse (from leaf to core)
            // For DOWN/CORE segments, we traverse forward (as stored)
            let segment_hops: Vec<HopInfo> = if is_up {
                // UP: traverse in reverse, swap ingress/egress
                entries.iter().rev().map(|e| {
                    HopInfo::new(
                        e.isd_as,
                        e.hop_entry.egress,  // egress becomes ingress when reversed
                        Some(e.hop_entry.ingress),  // ingress becomes egress when reversed
                    )
                }).collect()
            } else {
                // DOWN/CORE: traverse forward
                entries.iter().map(|e| {
                    HopInfo::new(
                        e.isd_as,
                        Some(e.hop_entry.ingress),
                        e.hop_entry.egress,
                    )
                }).collect()
            };

            if i == 0 {
                // First segment: include all hops
                hops.extend(segment_hops);
            } else if !segment_hops.is_empty() {
                // Subsequent segments: merge junction hop, then add rest
                // The junction AS appears at the end of the previous segment
                // and the start of this segment - merge their interfaces
                if let Some(last_hop) = hops.last_mut() {
                    // Update the junction hop's egress from this segment's first hop
                    let junction_hop = &segment_hops[0];
                    last_hop.egress = junction_hop.egress;
                }
                // Add remaining hops (skip junction)
                hops.extend(segment_hops.into_iter().skip(1));
            }
        }

        // Fix up first and last hops
        if let Some(first) = hops.first_mut() {
            first.ingress = None; // Source AS has no ingress
        }
        if let Some(last) = hops.last_mut() {
            last.egress = None; // Destination AS has no egress
        }

        hops
    }

    /// Calculate path length with peering shortcut
    fn calculate_shortcut_length(
        segments: &[PathSegmentRef],
        shortcut: &PeeringShortcut,
    ) -> usize {
        if segments.is_empty() {
            return 0;
        }

        // Find the up segment and down segment
        let up_seg = segments.iter().find(|s| s.segment_type() == SegmentType::Up);
        let down_seg = segments.iter().find(|s| s.segment_type() == SegmentType::Down);

        let mut length = 0;

        // Add length from up segment up to the peering point
        if let Some(up_ref) = up_seg {
            let up_cutoff = up_ref
                .segment()
                .pcb
                .as_entries
                .iter()
                .position(|e| e.isd_as == shortcut.up_side_as)
                .unwrap_or(up_ref.len() - 1);
            length += up_cutoff + 1; // Include the peering AS
        }

        // Add the peering hop (already counted in up_cutoff + 1)

        // Add length from peering point to destination in down segment
        if let Some(down_ref) = down_seg {
            let down_start = down_ref
                .segment()
                .pcb
                .as_entries
                .iter()
                .position(|e| e.isd_as == shortcut.down_side_as)
                .unwrap_or(0);
            length += down_ref.len() - down_start; // Remaining hops
        }

        length
    }

    /// Validate that this path is well-formed
    pub fn validate(&self) -> Result<(), String> {
        // Intra-AS paths are always valid
        if self.is_intra_as() {
            return Ok(());
        }

        // Check that segments are non-empty
        if self.segments.is_empty() {
            return Err("Path has no segments".to_string());
        }

        // Check that all segments are terminated
        for seg_ref in &self.segments {
            if !seg_ref.segment().is_terminated() {
                return Err("Path contains non-terminated segment".to_string());
            }
        }

        // Helper to get logical src/dst accounting for segment directionality
        // UP segments: traversed in reverse (non-core -> core)
        // DOWN/CORE segments: traversed forward (src -> dst)
        let logical_src = |seg_ref: &PathSegmentRef| -> Option<IsdAs> {
            match seg_ref {
                PathSegmentRef::Up(seg) => seg.dst(), // Reversed
                _ => seg_ref.segment().src(),
            }
        };
        let logical_dst = |seg_ref: &PathSegmentRef| -> Option<IsdAs> {
            match seg_ref {
                PathSegmentRef::Up(seg) => seg.src(), // Reversed
                _ => seg_ref.segment().dst(),
            }
        };

        // Check that segments connect properly
        for i in 0..self.segments.len() - 1 {
            let seg1_dst = logical_dst(&self.segments[i]).ok_or("Segment has no destination")?;
            let seg2_src = logical_src(&self.segments[i + 1]).ok_or("Segment has no source")?;

            if seg1_dst != seg2_src {
                return Err(format!(
                    "Segments do not connect: {} -> {}",
                    seg1_dst, seg2_src
                ));
            }
        }

        // Check that first segment starts at source
        let first_src = logical_src(&self.segments[0]).ok_or("First segment has no source")?;
        if first_src != self.src {
            return Err(format!(
                "Path source mismatch: expected {}, got {}",
                self.src, first_src
            ));
        }

        // Check that last segment ends at destination
        let last_dst = logical_dst(self.segments.last().unwrap()).ok_or("Last segment has no destination")?;
        if last_dst != self.dst {
            return Err(format!(
                "Path destination mismatch: expected {}, got {}",
                self.dst, last_dst
            ));
        }

        Ok(())
    }
}

/// Result of a path query
///
/// Contains the original query and either a list of paths or an error.
#[derive(Debug, Clone)]
pub struct PathQueryResult {
    /// Query that produced this result
    pub query: PathQuery,

    /// Paths found (may be empty)
    pub paths: Vec<ScionPath>,

    /// Error if query failed
    pub error: Option<String>,
}

impl PathQueryResult {
    /// Create a successful result
    pub fn success(query: PathQuery, paths: Vec<ScionPath>) -> Self {
        Self {
            query,
            paths,
            error: None,
        }
    }

    /// Create a failed result
    pub fn error(query: PathQuery, error: String) -> Self {
        Self {
            query,
            paths: Vec::new(),
            error: Some(error),
        }
    }

    /// Check if the query succeeded
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Check if the query failed
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Get the number of paths found
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// Check if any paths were found
    pub fn has_paths(&self) -> bool {
        !self.paths.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scion::pcb::{AsEntry, HopEntry, Pcb};

    fn create_test_segment(seg_type: SegmentType, src: IsdAs, dst: IsdAs) -> Arc<PathSegment> {
        // For UP segments: PCB is in beaconing direction (core->non-core)
        // So we need to reverse src/dst for the PCB construction
        // For DOWN/CORE: PCB matches routing direction
        let (pcb_start, pcb_end) = if seg_type == SegmentType::Up {
            (dst, src) // Reversed: core first, then non-core
        } else {
            (src, dst) // Normal: src first, then dst
        };

        let mut pcb = Pcb::new(pcb_start);
        pcb.extend(AsEntry::new(
            pcb_start,
            Some(pcb_end),
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        pcb.extend(AsEntry::new(
            pcb_end,
            None,
            HopEntry::new(InterfaceId::new(1), None),
        ));
        Arc::new(PathSegment::new(seg_type, pcb))
    }

    #[test]
    fn test_path_query_builder() {
        let src = IsdAs::new(1, 100);
        let dst = IsdAs::new(1, 200);

        let query = PathQuery::new(src, dst)
            .with_max_paths(5)
            .with_peering(false);

        assert_eq!(query.src, src);
        assert_eq!(query.dst, dst);
        assert_eq!(query.max_paths, 5);
        assert!(!query.allow_peering);
    }

    #[test]
    fn test_intra_as_path() {
        let isd_as = IsdAs::new(1, 100);
        let path = ScionPath::intra_as(isd_as);

        assert_eq!(path.src, isd_as);
        assert_eq!(path.dst, isd_as);
        assert!(path.is_intra_as());
        assert_eq!(path.total_length, 0);
        assert_eq!(path.segment_count(), 0);
        assert!(path.validate().is_ok());
    }

    #[test]
    fn test_simple_path() {
        let src = IsdAs::new(1, 100);
        let core = IsdAs::new(1, 200);
        let dst = IsdAs::new(1, 300);

        let up_seg = create_test_segment(SegmentType::Up, src, core);
        let down_seg = create_test_segment(SegmentType::Down, core, dst);

        let path = ScionPath::new(
            src,
            dst,
            vec![
                PathSegmentRef::Up(up_seg),
                PathSegmentRef::Down(down_seg),
            ],
            None,
        );

        assert_eq!(path.src, src);
        assert_eq!(path.dst, dst);
        assert_eq!(path.segment_count(), 2);
        assert!(!path.is_intra_as());
        assert!(!path.uses_peering());
        assert_eq!(path.total_length, 4); // 2 hops per segment
        assert!(path.validate().is_ok());
    }

    #[test]
    fn test_path_segment_ref() {
        let src = IsdAs::new(1, 100);
        let dst = IsdAs::new(1, 200);

        let seg = create_test_segment(SegmentType::Up, src, dst);
        let seg_ref = PathSegmentRef::Up(seg);

        assert_eq!(seg_ref.segment_type(), SegmentType::Up);
        assert_eq!(seg_ref.len(), 2);
        assert!(!seg_ref.is_empty());
    }

    #[test]
    fn test_path_as_path() {
        let src = IsdAs::new(1, 100);
        let core = IsdAs::new(1, 200);
        let dst = IsdAs::new(1, 300);

        let up_seg = create_test_segment(SegmentType::Up, src, core);
        let down_seg = create_test_segment(SegmentType::Down, core, dst);

        let path = ScionPath::new(
            src,
            dst,
            vec![
                PathSegmentRef::Up(up_seg),
                PathSegmentRef::Down(down_seg),
            ],
            None,
        );

        let as_path = path.as_path();
        assert_eq!(as_path, vec![src, core, dst]);
        // Note: as_path() automatically deduplicates overlapping ASes between segments
    }

    #[test]
    fn test_path_query_result() {
        let src = IsdAs::new(1, 100);
        let dst = IsdAs::new(1, 200);
        let query = PathQuery::new(src, dst);

        let result = PathQueryResult::success(query.clone(), vec![]);
        assert!(result.is_success());
        assert!(!result.is_error());
        assert!(!result.has_paths());
        assert_eq!(result.path_count(), 0);

        let error_result = PathQueryResult::error(query, "No path found".to_string());
        assert!(!error_result.is_success());
        assert!(error_result.is_error());
    }
}
