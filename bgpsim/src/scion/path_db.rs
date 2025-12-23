// Path Database for registered path segments (OPTIMIZED FOR SCALE)
//
// The path database stores registered path segments (up, down, core)
// that can be used for path lookup and combination.
//
// Optimizations:
// - Arc<PathSegment> for zero-copy sharing
// - HashMap indexes for O(1) lookups by src/dst
// - Efficient queries without full scans

use super::pcb::Pcb;
use super::types::IsdAs;
use std::collections::HashMap;
use std::sync::Arc;

/// Path segment types
///
/// SCION uses three types of path segments that are combined to form
/// end-to-end paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegmentType {
    /// Up segment: from non-core AS to core AS
    Up,

    /// Down segment: from core AS to non-core AS
    Down,

    /// Core segment: between core ASes (intra-ISD or inter-ISD)
    Core,
}

impl SegmentType {
    pub fn is_up(&self) -> bool {
        matches!(self, Self::Up)
    }

    pub fn is_down(&self) -> bool {
        matches!(self, Self::Down)
    }

    pub fn is_core(&self) -> bool {
        matches!(self, Self::Core)
    }
}

impl std::fmt::Display for SegmentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Up => write!(f, "Up"),
            Self::Down => write!(f, "Down"),
            Self::Core => write!(f, "Core"),
        }
    }
}

/// Registered path segment
///
/// A path segment is a "snapshot" of a PCB at a given time from a particular
/// AS's vantage point. It is created by terminating a PCB (setting next_isd_as
/// and egress to None/0 in the last entry).
#[derive(Debug, Clone)]
pub struct PathSegment {
    pub segment_type: SegmentType,
    pub pcb: Pcb,
}

impl PathSegment {
    /// Create a new path segment
    pub fn new(segment_type: SegmentType, pcb: Pcb) -> Self {
        Self { segment_type, pcb }
    }

    /// Get the source ISD-AS (first entry)
    pub fn src(&self) -> Option<IsdAs> {
        self.pcb.src()
    }

    /// Get the destination ISD-AS (last entry)
    pub fn dst(&self) -> Option<IsdAs> {
        self.pcb.dst()
    }

    /// Get the length of the segment (number of AS entries)
    pub fn len(&self) -> usize {
        self.pcb.len()
    }

    /// Check if the segment is empty
    pub fn is_empty(&self) -> bool {
        self.pcb.is_empty()
    }

    /// Check if this segment is terminated
    ///
    /// A properly formed path segment should always be terminated
    /// (last entry has no next_isd_as).
    pub fn is_terminated(&self) -> bool {
        self.pcb.is_terminated()
    }

    /// Validate that this segment is well-formed
    pub fn validate(&self) -> Result<(), String> {
        if self.pcb.is_empty() {
            return Err("Segment has no AS entries".to_string());
        }

        if !self.is_terminated() {
            return Err("Segment is not terminated".to_string());
        }

        // Additional validation based on segment type
        match self.segment_type {
            SegmentType::Up => {
                // Up segments should start at non-core and end at core
                // (validation of core status would require network context)
            }
            SegmentType::Down => {
                // Down segments should start at core and end at non-core
            }
            SegmentType::Core => {
                // Core segments should be between core ASes
            }
        }

        Ok(())
    }

    /// Get the AS-level path
    pub fn as_path(&self) -> Vec<IsdAs> {
        self.pcb.as_path()
    }
}

/// Permanent storage for registered path segments (OPTIMIZED)
///
/// Each AS maintains a path database containing:
/// - Up segments: paths to core ASes (stored locally by non-core ASes)
/// - Down segments: paths from cores to non-cores (registered at cores)
/// - Core segments: paths between cores (stored by core ASes)
///
/// Uses Arc<PathSegment> for memory efficiency and HashMap indexes for O(1) lookups.
#[derive(Debug, Clone)]
pub struct PathDatabase {
    // Up segments storage and indexes
    up_segments: Vec<Arc<PathSegment>>,
    up_by_dst: HashMap<IsdAs, Vec<usize>>,  // dst -> indices in up_segments

    // Down segments storage and indexes
    down_segments: Vec<Arc<PathSegment>>,
    down_by_src: HashMap<IsdAs, Vec<usize>>,  // src -> indices in down_segments
    down_by_dst: HashMap<IsdAs, Vec<usize>>,  // dst -> indices in down_segments

    // Core segments storage and indexes
    core_segments: Vec<Arc<PathSegment>>,
    core_by_src: HashMap<IsdAs, Vec<usize>>,  // src -> indices in core_segments
    core_by_pair: HashMap<(IsdAs, IsdAs), Vec<usize>>,  // (src, dst) -> indices in core_segments
}

impl Default for PathDatabase {
    fn default() -> Self {
        Self::new()
    }
}

impl PathDatabase {
    /// Create a new empty path database
    pub fn new() -> Self {
        Self {
            up_segments: Vec::new(),
            up_by_dst: HashMap::new(),
            down_segments: Vec::new(),
            down_by_src: HashMap::new(),
            down_by_dst: HashMap::new(),
            core_segments: Vec::new(),
            core_by_src: HashMap::new(),
            core_by_pair: HashMap::new(),
        }
    }

    /// Create a new path database with pre-allocated capacity
    ///
    /// Use this when you know approximately how many segments will be added.
    /// Avoids reallocations during bulk insertion.
    pub fn with_capacity(up: usize, down: usize, core: usize) -> Self {
        Self {
            up_segments: Vec::with_capacity(up),
            up_by_dst: HashMap::with_capacity(up),
            down_segments: Vec::with_capacity(down),
            down_by_src: HashMap::with_capacity(down),
            down_by_dst: HashMap::with_capacity(down),
            core_segments: Vec::with_capacity(core),
            core_by_src: HashMap::with_capacity(core),
            core_by_pair: HashMap::with_capacity(core),
        }
    }

    /// Add a path segment to the database
    pub fn add_segment(&mut self, segment: Arc<PathSegment>) {
        match segment.segment_type {
            SegmentType::Up => {
                let idx = self.up_segments.len();
                // Up segments: PCB is [core -> non-core (term)]
                // We index by src (the core AS we can reach)
                if let Some(src) = segment.src() {
                    self.up_by_dst.entry(src).or_default().push(idx);
                }
                self.up_segments.push(segment);
            }
            SegmentType::Down => {
                let idx = self.down_segments.len();
                if let Some(src) = segment.src() {
                    self.down_by_src.entry(src).or_default().push(idx);
                }
                if let Some(dst) = segment.dst() {
                    self.down_by_dst.entry(dst).or_default().push(idx);
                }
                self.down_segments.push(segment);
            }
            SegmentType::Core => {
                let idx = self.core_segments.len();
                if let Some(src) = segment.src() {
                    self.core_by_src.entry(src).or_default().push(idx);
                }
                if let (Some(src), Some(dst)) = (segment.src(), segment.dst()) {
                    self.core_by_pair.entry((src, dst)).or_default().push(idx);
                }
                self.core_segments.push(segment);
            }
        }
    }

    /// Get all up segments to a specific destination (O(1) indexed lookup)
    pub fn get_up_to(&self, dst: IsdAs) -> Vec<Arc<PathSegment>> {
        self.up_by_dst
            .get(&dst)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| self.up_segments.get(idx).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all down segments from a specific source (O(1) indexed lookup)
    pub fn get_down_from(&self, src: IsdAs) -> Vec<Arc<PathSegment>> {
        self.down_by_src
            .get(&src)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| self.down_segments.get(idx).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all down segments to a specific destination (O(1) indexed lookup)
    pub fn get_down_to(&self, dst: IsdAs) -> Vec<Arc<PathSegment>> {
        self.down_by_dst
            .get(&dst)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| self.down_segments.get(idx).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get core segments between two ASes (O(1) indexed lookup)
    pub fn get_core(&self, src: IsdAs, dst: IsdAs) -> Vec<Arc<PathSegment>> {
        self.core_by_pair
            .get(&(src, dst))
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| self.core_segments.get(idx).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all core segments from a specific source (O(1) indexed lookup)
    pub fn get_core_from(&self, src: IsdAs) -> Vec<Arc<PathSegment>> {
        self.core_by_src
            .get(&src)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| self.core_segments.get(idx).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all segments (of any type) - allocates a new Vec
    ///
    /// For bulk operations, prefer `iter_all()` to avoid allocation.
    pub fn get_all(&self) -> Vec<Arc<PathSegment>> {
        self.up_segments
            .iter()
            .chain(self.down_segments.iter())
            .chain(self.core_segments.iter())
            .cloned()
            .collect()
    }

    /// Iterate over all segments without allocation
    ///
    /// More efficient than `get_all()` for bulk operations since it
    /// doesn't allocate a new Vec.
    pub fn iter_all(&self) -> impl Iterator<Item = &Arc<PathSegment>> {
        self.up_segments
            .iter()
            .chain(self.down_segments.iter())
            .chain(self.core_segments.iter())
    }

    /// Extend this database with all segments from another database
    ///
    /// More efficient than iterating and calling add_segment() individually
    /// because it avoids intermediate allocations.
    pub fn extend_from(&mut self, other: &PathDatabase) {
        // Pre-extend capacity
        self.up_segments.reserve(other.up_segments.len());
        self.down_segments.reserve(other.down_segments.len());
        self.core_segments.reserve(other.core_segments.len());

        // Add all segments (indexes are rebuilt automatically)
        for segment in other.iter_all() {
            self.add_segment(segment.clone());
        }
    }

    /// Get total number of segments
    pub fn total_segments(&self) -> usize {
        self.up_segments.len() + self.down_segments.len() + self.core_segments.len()
    }

    /// Check if the database is empty
    pub fn is_empty(&self) -> bool {
        self.total_segments() == 0
    }

    /// Get all up segments originating from a specific source AS
    ///
    /// This returns up segments where the source AS is the starting point.
    /// Note: In SCION, up segments go from non-core to core, so this finds
    /// segments starting at the given non-core AS.
    pub fn get_up_from(&self, src: IsdAs) -> Vec<Arc<PathSegment>> {
        self.up_segments
            .iter()
            .filter(|seg| seg.dst() == Some(src))
            .cloned()
            .collect()
    }

    /// Get down segments from a specific core to a specific destination
    ///
    /// This is more specific than get_down_from, filtering by both src and dst.
    pub fn get_down_from_to(&self, core: IsdAs, dst: IsdAs) -> Vec<Arc<PathSegment>> {
        self.down_by_src
            .get(&core)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| self.down_segments.get(idx))
                    .filter(|seg| seg.dst() == Some(dst))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get core segments from source to any AS in destination ISD
    ///
    /// This is useful for inter-ISD routing where we need to reach any core
    /// in the destination ISD.
    pub fn get_core_to_isd(&self, src: IsdAs, dst_isd: u16) -> Vec<Arc<PathSegment>> {
        self.core_by_src
            .get(&src)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&idx| self.core_segments.get(idx))
                    .filter(|seg| seg.dst().map(|d| d.isd.0) == Some(dst_isd))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Clear all segments from the database
    pub fn clear(&mut self) {
        self.up_segments.clear();
        self.up_by_dst.clear();
        self.down_segments.clear();
        self.down_by_src.clear();
        self.down_by_dst.clear();
        self.core_segments.clear();
        self.core_by_src.clear();
        self.core_by_pair.clear();
    }

    /// Get segment counts by type (up, down, core)
    pub fn segment_counts(&self) -> (usize, usize, usize) {
        (
            self.up_segments.len(),
            self.down_segments.len(),
            self.core_segments.len(),
        )
    }

    /// Remove segments older than a given timestamp
    pub fn remove_expired(&mut self, min_timestamp: i64) -> usize {
        let mut removed = 0;

        // Remove expired up segments
        let old_len = self.up_segments.len();
        self.up_segments
            .retain(|s| s.pcb.segment_info.timestamp >= min_timestamp);
        removed += old_len - self.up_segments.len();

        // Remove expired down segments
        let old_len = self.down_segments.len();
        self.down_segments
            .retain(|s| s.pcb.segment_info.timestamp >= min_timestamp);
        removed += old_len - self.down_segments.len();

        // Remove expired core segments
        let old_len = self.core_segments.len();
        self.core_segments
            .retain(|s| s.pcb.segment_info.timestamp >= min_timestamp);
        removed += old_len - self.core_segments.len();

        // Rebuild indexes if anything was removed
        if removed > 0 {
            self.rebuild_indexes();
        }

        removed
    }

    /// Rebuild all indexes from scratch
    fn rebuild_indexes(&mut self) {
        // Clear all indexes
        self.up_by_dst.clear();
        self.down_by_src.clear();
        self.down_by_dst.clear();
        self.core_by_src.clear();
        self.core_by_pair.clear();

        // Rebuild up indexes
        for (idx, segment) in self.up_segments.iter().enumerate() {
            if let Some(dst) = segment.dst() {
                self.up_by_dst.entry(dst).or_default().push(idx);
            }
        }

        // Rebuild down indexes
        for (idx, segment) in self.down_segments.iter().enumerate() {
            if let Some(src) = segment.src() {
                self.down_by_src.entry(src).or_default().push(idx);
            }
            if let Some(dst) = segment.dst() {
                self.down_by_dst.entry(dst).or_default().push(idx);
            }
        }

        // Rebuild core indexes
        for (idx, segment) in self.core_segments.iter().enumerate() {
            if let Some(src) = segment.src() {
                self.core_by_src.entry(src).or_default().push(idx);
            }
            if let (Some(src), Some(dst)) = (segment.src(), segment.dst()) {
                self.core_by_pair.entry((src, dst)).or_default().push(idx);
            }
        }
    }

    /// Get statistics about the database
    pub fn stats(&self) -> DatabaseStats {
        DatabaseStats {
            up_count: self.up_segments.len(),
            down_count: self.down_segments.len(),
            core_count: self.core_segments.len(),
            total_count: self.total_segments(),
        }
    }
}

/// Statistics about a path database
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatabaseStats {
    pub up_count: usize,
    pub down_count: usize,
    pub core_count: usize,
    pub total_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scion::pcb::{AsEntry, HopEntry, SegmentInfo};
    use crate::scion::types::InterfaceId;

    fn create_test_segment(
        segment_type: SegmentType,
        src: IsdAs,
        dst: IsdAs,
        intermediate_hops: usize,
    ) -> Arc<PathSegment> {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000, 42));

        // Add source entry
        pcb.extend(AsEntry::new(
            src,
            if intermediate_hops > 0 {
                Some(IsdAs::new(src.isd.as_u16(), src.asn.0 + 1))
            } else {
                Some(dst)
            },
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));

        // Add intermediate hops
        for i in 0..intermediate_hops {
            let current = IsdAs::new(src.isd.as_u16(), src.asn.0 + 1 + i as u32);
            let next = if i < intermediate_hops - 1 {
                IsdAs::new(src.isd.as_u16(), src.asn.0 + 2 + i as u32)
            } else {
                dst
            };

            pcb.extend(AsEntry::new(
                current,
                Some(next),
                HopEntry::new(InterfaceId::new((i + 1) as u16), Some(InterfaceId::new((i + 2) as u16))),
            ));
        }

        // Add destination entry (terminated)
        pcb.extend(AsEntry::new(
            dst,
            None,  // Terminated
            HopEntry::new(InterfaceId::new((intermediate_hops + 1) as u16), None),
        ));

        Arc::new(PathSegment::new(segment_type, pcb))
    }

    #[test]
    fn test_segment_type() {
        assert!(SegmentType::Up.is_up());
        assert!(!SegmentType::Up.is_down());
        assert!(!SegmentType::Up.is_core());

        assert!(SegmentType::Down.is_down());
        assert!(SegmentType::Core.is_core());
    }

    #[test]
    fn test_segment_type_display() {
        assert_eq!(SegmentType::Up.to_string(), "Up");
        assert_eq!(SegmentType::Down.to_string(), "Down");
        assert_eq!(SegmentType::Core.to_string(), "Core");
    }

    #[test]
    fn test_path_segment_creation() {
        let src = IsdAs::new(1, 100);
        let dst = IsdAs::new(1, 200);
        let segment = create_test_segment(SegmentType::Up, src, dst, 1);

        assert_eq!(segment.segment_type, SegmentType::Up);
        assert_eq!(segment.src(), Some(src));
        assert_eq!(segment.dst(), Some(dst));
        assert_eq!(segment.len(), 3); // src + 1 intermediate + dst
        assert!(segment.is_terminated());
    }

    #[test]
    fn test_path_segment_validation() {
        let src = IsdAs::new(1, 100);
        let dst = IsdAs::new(1, 200);
        let segment = create_test_segment(SegmentType::Up, src, dst, 0);

        assert!(segment.validate().is_ok());

        // Test empty segment
        let empty_segment = PathSegment::new(SegmentType::Up, Pcb::new(src));
        assert!(empty_segment.validate().is_err());

        // Test non-terminated segment
        let mut unterminated_pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000, 42));
        unterminated_pcb.extend(AsEntry::new(
            src,
            Some(dst),  // Not terminated
            HopEntry::new(InterfaceId::ZERO, Some(InterfaceId::new(1))),
        ));
        let unterminated = PathSegment::new(SegmentType::Up, unterminated_pcb);
        assert!(unterminated.validate().is_err());
    }

    #[test]
    fn test_path_database_creation() {
        let db = PathDatabase::new();
        assert!(db.is_empty());
        assert_eq!(db.total_segments(), 0);
    }

    #[test]
    fn test_add_segments() {
        let mut db = PathDatabase::new();

        let up = create_test_segment(SegmentType::Up, IsdAs::new(1, 100), IsdAs::new(1, 200), 0);
        let down = create_test_segment(SegmentType::Down, IsdAs::new(1, 200), IsdAs::new(1, 101), 0);
        let core = create_test_segment(SegmentType::Core, IsdAs::new(1, 200), IsdAs::new(1, 201), 0);

        db.add_segment(up);
        db.add_segment(down);
        db.add_segment(core);

        assert_eq!(db.total_segments(), 3);
        assert_eq!(db.up_segments.len(), 1);
        assert_eq!(db.down_segments.len(), 1);
        assert_eq!(db.core_segments.len(), 1);
    }

    #[test]
    fn test_get_up_to() {
        let mut db = PathDatabase::new();

        let core1 = IsdAs::new(1, 200);
        let core2 = IsdAs::new(1, 201);

        // Up segments: PCB is [core -> non-core (term)]
        // We query by core AS (src of PCB)
        db.add_segment(create_test_segment(SegmentType::Up, core1, IsdAs::new(1, 100), 0));
        db.add_segment(create_test_segment(SegmentType::Up, core1, IsdAs::new(1, 101), 0));
        db.add_segment(create_test_segment(SegmentType::Up, core2, IsdAs::new(1, 102), 0));

        let to_core1 = db.get_up_to(core1);
        assert_eq!(to_core1.len(), 2);

        let to_core2 = db.get_up_to(core2);
        assert_eq!(to_core2.len(), 1);

        let to_nonexistent = db.get_up_to(IsdAs::new(1, 999));
        assert_eq!(to_nonexistent.len(), 0);
    }

    #[test]
    fn test_get_down_from() {
        let mut db = PathDatabase::new();

        let src1 = IsdAs::new(1, 200);
        let src2 = IsdAs::new(1, 201);

        db.add_segment(create_test_segment(SegmentType::Down, src1, IsdAs::new(1, 100), 0));
        db.add_segment(create_test_segment(SegmentType::Down, src1, IsdAs::new(1, 101), 0));
        db.add_segment(create_test_segment(SegmentType::Down, src2, IsdAs::new(1, 102), 0));

        let from_src1 = db.get_down_from(src1);
        assert_eq!(from_src1.len(), 2);

        let from_src2 = db.get_down_from(src2);
        assert_eq!(from_src2.len(), 1);
    }

    #[test]
    fn test_get_down_to() {
        let mut db = PathDatabase::new();

        let dst1 = IsdAs::new(1, 100);
        let dst2 = IsdAs::new(1, 101);

        db.add_segment(create_test_segment(SegmentType::Down, IsdAs::new(1, 200), dst1, 0));
        db.add_segment(create_test_segment(SegmentType::Down, IsdAs::new(1, 201), dst1, 0));
        db.add_segment(create_test_segment(SegmentType::Down, IsdAs::new(1, 202), dst2, 0));

        let to_dst1 = db.get_down_to(dst1);
        assert_eq!(to_dst1.len(), 2);

        let to_dst2 = db.get_down_to(dst2);
        assert_eq!(to_dst2.len(), 1);
    }

    #[test]
    fn test_get_core() {
        let mut db = PathDatabase::new();

        let src1 = IsdAs::new(1, 200);
        let src2 = IsdAs::new(1, 201);
        let dst1 = IsdAs::new(1, 202);
        let dst2 = IsdAs::new(2, 200);

        db.add_segment(create_test_segment(SegmentType::Core, src1, dst1, 0));
        db.add_segment(create_test_segment(SegmentType::Core, src1, dst2, 0));
        db.add_segment(create_test_segment(SegmentType::Core, src2, dst1, 0));

        let s1_to_d1 = db.get_core(src1, dst1);
        assert_eq!(s1_to_d1.len(), 1);

        let s1_to_d2 = db.get_core(src1, dst2);
        assert_eq!(s1_to_d2.len(), 1);

        let s2_to_d1 = db.get_core(src2, dst1);
        assert_eq!(s2_to_d1.len(), 1);

        let nonexistent = db.get_core(src2, dst2);
        assert_eq!(nonexistent.len(), 0);
    }

    #[test]
    fn test_get_core_from() {
        let mut db = PathDatabase::new();

        let src1 = IsdAs::new(1, 200);
        let src2 = IsdAs::new(1, 201);

        db.add_segment(create_test_segment(SegmentType::Core, src1, IsdAs::new(1, 202), 0));
        db.add_segment(create_test_segment(SegmentType::Core, src1, IsdAs::new(2, 200), 0));
        db.add_segment(create_test_segment(SegmentType::Core, src2, IsdAs::new(1, 202), 0));

        let from_src1 = db.get_core_from(src1);
        assert_eq!(from_src1.len(), 2);

        let from_src2 = db.get_core_from(src2);
        assert_eq!(from_src2.len(), 1);
    }

    #[test]
    fn test_get_all() {
        let mut db = PathDatabase::new();

        db.add_segment(create_test_segment(SegmentType::Up, IsdAs::new(1, 100), IsdAs::new(1, 200), 0));
        db.add_segment(create_test_segment(SegmentType::Down, IsdAs::new(1, 200), IsdAs::new(1, 101), 0));
        db.add_segment(create_test_segment(SegmentType::Core, IsdAs::new(1, 200), IsdAs::new(1, 201), 0));

        let all = db.get_all();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_clear() {
        let mut db = PathDatabase::new();

        db.add_segment(create_test_segment(SegmentType::Up, IsdAs::new(1, 100), IsdAs::new(1, 200), 0));
        db.add_segment(create_test_segment(SegmentType::Down, IsdAs::new(1, 200), IsdAs::new(1, 101), 0));
        db.add_segment(create_test_segment(SegmentType::Core, IsdAs::new(1, 200), IsdAs::new(1, 201), 0));

        assert_eq!(db.total_segments(), 3);

        db.clear();
        assert!(db.is_empty());
        assert_eq!(db.total_segments(), 0);
    }

    #[test]
    fn test_remove_expired() {
        let mut db = PathDatabase::new();

        // Create segments with different timestamps
        let mut old_pcb = Pcb::with_segment_info(SegmentInfo::with_values(1000, 1));
        old_pcb.extend(AsEntry::new(
            IsdAs::new(1, 100),
            None,
            HopEntry::new(InterfaceId::ZERO, None),
        ));

        let mut new_pcb = Pcb::with_segment_info(SegmentInfo::with_values(3000, 2));
        new_pcb.extend(AsEntry::new(
            IsdAs::new(1, 101),
            None,
            HopEntry::new(InterfaceId::ZERO, None),
        ));

        db.add_segment(Arc::new(PathSegment::new(SegmentType::Up, old_pcb)));
        db.add_segment(Arc::new(PathSegment::new(SegmentType::Up, new_pcb)));

        assert_eq!(db.total_segments(), 2);

        let removed = db.remove_expired(2000);
        assert_eq!(removed, 1);
        assert_eq!(db.total_segments(), 1);
    }

    #[test]
    fn test_stats() {
        let mut db = PathDatabase::new();

        db.add_segment(create_test_segment(SegmentType::Up, IsdAs::new(1, 100), IsdAs::new(1, 200), 0));
        db.add_segment(create_test_segment(SegmentType::Up, IsdAs::new(1, 101), IsdAs::new(1, 200), 0));
        db.add_segment(create_test_segment(SegmentType::Down, IsdAs::new(1, 200), IsdAs::new(1, 102), 0));
        db.add_segment(create_test_segment(SegmentType::Core, IsdAs::new(1, 200), IsdAs::new(1, 201), 0));

        let stats = db.stats();
        assert_eq!(stats.up_count, 2);
        assert_eq!(stats.down_count, 1);
        assert_eq!(stats.core_count, 1);
        assert_eq!(stats.total_count, 4);
    }
}
