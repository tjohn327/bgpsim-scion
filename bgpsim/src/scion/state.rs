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

//! SCION control plane state management including beacon storage and path database.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::Prefix;

use super::{
    path_segment::{PathSegment, SegmentType},
    pcb::Pcb,
    types::IsdAs,
};

/// Storage for Path Construction Beacons (PCBs).
///
/// The BeaconStore maintains PCBs received from neighbors, organizing them
/// by source AS and managing capacity limits and expiration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "P: Prefix + Serialize + for<'d> Deserialize<'d>")]
pub struct BeaconStore<P: Prefix> {
    /// PCBs indexed by the originating core AS
    beacons: HashMap<IsdAs, Vec<Pcb<P>>>,
    /// Maximum number of PCBs to store per source AS
    max_per_source: usize,
    /// Maximum total number of PCBs to store
    max_total: usize,
}

impl<P: Prefix> BeaconStore<P> {
    /// Create a new beacon store with specified capacity limits.
    ///
    /// # Arguments
    /// * `max_per_source` - Maximum PCBs to store from each source AS (default: 20)
    /// * `max_total` - Maximum total PCBs to store (default: 1000)
    pub fn new(max_per_source: usize, max_total: usize) -> Self {
        BeaconStore {
            beacons: HashMap::new(),
            max_per_source,
            max_total,
        }
    }

    /// Create a new beacon store with default capacity limits.
    pub fn default() -> Self {
        Self::new(20, 1000)
    }

    /// Insert a PCB into the store.
    ///
    /// If the store is at capacity, the oldest/lowest quality PCB may be evicted.
    /// Returns true if the PCB was inserted, false if it was rejected.
    pub fn insert(&mut self, pcb: Pcb<P>) -> bool {
        // Get the origin AS (first AS in the path)
        let origin = match pcb.get_origin() {
            Some(origin) => origin,
            None => return false, // Empty PCB, reject
        };

        // Check total capacity
        if self.total_count() >= self.max_total && !self.beacons.contains_key(&origin) {
            // At capacity and this is a new source - reject
            return false;
        }

        // Get or create the vector for this source
        let pcbs = self.beacons.entry(origin).or_insert_with(Vec::new);

        // Check per-source capacity
        if pcbs.len() >= self.max_per_source {
            // At capacity for this source - could implement eviction policy here
            // For now, just reject
            return false;
        }

        // Insert the PCB
        pcbs.push(pcb);
        true
    }

    /// Get all PCBs from a specific source AS.
    pub fn get_by_source(&self, source: &IsdAs) -> Vec<&Pcb<P>> {
        self.beacons
            .get(source)
            .map(|pcbs| pcbs.iter().collect())
            .unwrap_or_default()
    }

    /// Get all PCBs in the store.
    pub fn get_all(&self) -> Vec<&Pcb<P>> {
        self.beacons
            .values()
            .flat_map(|pcbs| pcbs.iter())
            .collect()
    }

    /// Remove expired PCBs based on current time.
    ///
    /// # Arguments
    /// * `current_time` - Current timestamp
    /// * `tolerance` - Additional tolerance for expiration (in seconds)
    ///
    /// Returns the number of PCBs removed.
    pub fn remove_expired(&mut self, current_time: u32, tolerance: u32) -> usize {
        let mut removed = 0;

        // Remove expired PCBs from each source
        for pcbs in self.beacons.values_mut() {
            let original_len = pcbs.len();
            pcbs.retain(|pcb| !pcb.is_expired(current_time, tolerance));
            removed += original_len - pcbs.len();
        }

        // Remove empty entries
        self.beacons.retain(|_, pcbs| !pcbs.is_empty());

        removed
    }

    /// Clear all PCBs from the store.
    pub fn clear(&mut self) {
        self.beacons.clear();
    }

    /// Get the total number of PCBs stored.
    pub fn total_count(&self) -> usize {
        self.beacons.values().map(|pcbs| pcbs.len()).sum()
    }

    /// Get the number of different source ASes.
    pub fn source_count(&self) -> usize {
        self.beacons.len()
    }

    /// Get all unique source ASes.
    pub fn sources(&self) -> Vec<IsdAs> {
        self.beacons.keys().copied().collect()
    }
}

/// Storage for registered path segments.
///
/// The PathDatabase maintains path segments that have been registered,
/// organizing them by segment type (up, down, core) and destination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound = "P: Prefix + Serialize + for<'d> Deserialize<'d>")]
pub struct PathDatabase<P: Prefix> {
    /// Up-segments: paths from non-core AS to core AS
    up_segments: Vec<PathSegment<P>>,
    /// Down-segments: paths from core AS to non-core AS
    down_segments: Vec<PathSegment<P>>,
    /// Core-segments: paths between core ASes
    core_segments: Vec<PathSegment<P>>,
    /// Maximum number of segments per type
    max_per_type: usize,
}

impl<P: Prefix> PathDatabase<P> {
    /// Create a new path database with specified capacity.
    pub fn new(max_per_type: usize) -> Self {
        PathDatabase {
            up_segments: Vec::new(),
            down_segments: Vec::new(),
            core_segments: Vec::new(),
            max_per_type,
        }
    }

    /// Create a new path database with default capacity.
    pub fn default() -> Self {
        Self::new(1000)
    }

    /// Register a path segment.
    ///
    /// The segment is added to the appropriate collection based on its type.
    /// Returns true if successful, false if at capacity.
    pub fn register_segment(&mut self, segment: PathSegment<P>) -> bool {
        let segments = match segment.segment_type {
            SegmentType::Up => &mut self.up_segments,
            SegmentType::Down => &mut self.down_segments,
            SegmentType::Core => &mut self.core_segments,
        };

        if segments.len() >= self.max_per_type {
            // At capacity - could implement eviction policy
            return false;
        }

        segments.push(segment);
        true
    }

    /// Lookup up-segments to a specific destination.
    ///
    /// # Arguments
    /// * `destination` - The destination ISD-AS (typically a core AS)
    ///
    /// Returns all up-segments that reach the destination.
    pub fn lookup_up_segments(&self, destination: &IsdAs) -> Vec<&PathSegment<P>> {
        self.up_segments
            .iter()
            .filter(|seg| seg.destination() == Some(*destination))
            .take(1000)  // Limit to prevent OOM (spec: line 1558 recommends max 50)
            .collect()
    }

    /// Lookup up-segments from a specific source.
    ///
    /// # Arguments
    /// * `source` - The source ISD-AS (typically a non-core AS)
    ///
    /// Returns all up-segments originating from the source.
    pub fn lookup_up_segments_from(&self, source: &IsdAs) -> Vec<&PathSegment<P>> {
        self.up_segments
            .iter()
            .filter(|seg| seg.source() == Some(*source))
            .take(1000)  // Limit to prevent OOM (spec: line 1558 recommends max 50)
            .collect()
    }

    /// Lookup down-segments from a specific source.
    ///
    /// # Arguments
    /// * `source` - The source ISD-AS (typically a core AS)
    ///
    /// Returns all down-segments originating from the source.
    pub fn lookup_down_segments(&self, source: &IsdAs) -> Vec<&PathSegment<P>> {
        self.down_segments
            .iter()
            .filter(|seg| seg.source() == Some(*source))
            .take(1000)  // Limit to prevent OOM (spec: line 1558 recommends max 50)
            .collect()
    }

    /// Lookup down-segments to a specific destination.
    ///
    /// # Arguments
    /// * `destination` - The destination ISD-AS
    ///
    /// Returns all down-segments that reach the destination.
    pub fn lookup_down_segments_to(&self, destination: &IsdAs) -> Vec<&PathSegment<P>> {
        self.down_segments
            .iter()
            .filter(|seg| seg.destination() == Some(*destination))
            .take(1000)  // Limit to prevent OOM (spec: line 1558 recommends max 50)
            .collect()
    }

    /// Lookup core-segments between two ISDs or ASes.
    ///
    /// # Arguments
    /// * `source` - Optional source ISD-AS filter
    /// * `destination` - Optional destination ISD-AS filter
    ///
    /// Returns all matching core-segments.
    pub fn lookup_core_segments(
        &self,
        source: Option<&IsdAs>,
        destination: Option<&IsdAs>,
    ) -> Vec<&PathSegment<P>> {
        self.core_segments
            .iter()
            .filter(|seg| {
                if let Some(src) = source {
                    if seg.source() != Some(*src) {
                        return false;
                    }
                }
                if let Some(dst) = destination {
                    if seg.destination() != Some(*dst) {
                        return false;
                    }
                }
                true
            })
            .take(1000)  // Limit to prevent OOM (spec: line 1558 recommends max 50)
            .collect()
    }

    /// Get all up-segments.
    pub fn get_all_up_segments(&self) -> &[PathSegment<P>] {
        &self.up_segments
    }

    /// Get all down-segments.
    pub fn get_all_down_segments(&self) -> &[PathSegment<P>] {
        &self.down_segments
    }

    /// Get all core-segments.
    pub fn get_all_core_segments(&self) -> &[PathSegment<P>] {
        &self.core_segments
    }

    /// Remove expired segments based on current time.
    ///
    /// Returns the number of segments removed.
    pub fn remove_expired(&mut self, current_time: u32) -> usize {
        let mut removed = 0;

        let before = self.up_segments.len();
        self.up_segments
            .retain(|seg| seg.expiration > current_time);
        removed += before - self.up_segments.len();

        let before = self.down_segments.len();
        self.down_segments
            .retain(|seg| seg.expiration > current_time);
        removed += before - self.down_segments.len();

        let before = self.core_segments.len();
        self.core_segments
            .retain(|seg| seg.expiration > current_time);
        removed += before - self.core_segments.len();

        removed
    }

    /// Clear all segments from the database.
    pub fn clear(&mut self) {
        self.up_segments.clear();
        self.down_segments.clear();
        self.core_segments.clear();
    }

    /// Get the total number of segments stored.
    pub fn total_count(&self) -> usize {
        self.up_segments.len() + self.down_segments.len() + self.core_segments.len()
    }

    /// Get all segments (up, down, and core) as a vector.
    pub fn get_all_segments(&self) -> Vec<PathSegment<P>> {
        let mut all_segments = Vec::new();
        all_segments.extend(self.up_segments.iter().cloned());
        all_segments.extend(self.down_segments.iter().cloned());
        all_segments.extend(self.core_segments.iter().cloned());
        all_segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SimplePrefix;

    use super::super::{
        pcb::{HopEntry, HopField, SegmentFlags, SegmentInfo},
        types::InterfaceId,
    };

    fn create_test_pcb(origin_as: u64, timestamp: u32) -> Pcb<SimplePrefix> {
        let info = SegmentInfo {
            timestamp,
            segment_id: 12345,
            flags: SegmentFlags { reserved: 0 },
        };
        let mut pcb = Pcb::new(info);

        let hop_entry = HopEntry {
            hop_field: HopField {
                ingress: InterfaceId::UNSPECIFIED,
                egress: InterfaceId(1),
                exp_time: 63,
                mac: 0x1234567890,
            },
            ingress_mtu: 1500,
        };

        pcb.add_as_entry(super::super::pcb::AsEntry::new(
            IsdAs::new(1, origin_as),
            hop_entry,
        ));

        pcb
    }

    fn create_test_segment(
        seg_type: SegmentType,
        source_as: u64,
        dest_as: u64,
    ) -> PathSegment<SimplePrefix> {
        let info = SegmentInfo {
            timestamp: 1000,
            segment_id: 12345,
            flags: SegmentFlags { reserved: 0 },
        };
        let mut pcb: Pcb<SimplePrefix> = Pcb::new(info);

        // Add source AS
        let hop1 = HopEntry {
            hop_field: HopField {
                ingress: InterfaceId::UNSPECIFIED,
                egress: InterfaceId(1),
                exp_time: 63,
                mac: 0x1234,
            },
            ingress_mtu: 1500,
        };
        pcb.add_as_entry(super::super::pcb::AsEntry::new(
            IsdAs::new(1, source_as),
            hop1,
        ));

        // Add destination AS
        let hop2 = HopEntry {
            hop_field: HopField {
                ingress: InterfaceId(2),
                egress: InterfaceId::UNSPECIFIED,
                exp_time: 63,
                mac: 0x5678,
            },
            ingress_mtu: 1500,
        };
        pcb.add_as_entry(super::super::pcb::AsEntry::new(
            IsdAs::new(1, dest_as),
            hop2,
        ));

        PathSegment::from_pcb(&pcb, seg_type)
    }

    #[test]
    fn test_beacon_store_creation() {
        let store: BeaconStore<SimplePrefix> = BeaconStore::new(10, 100);
        assert_eq!(store.total_count(), 0);
        assert_eq!(store.source_count(), 0);
    }

    #[test]
    fn test_beacon_store_insert() {
        let mut store = BeaconStore::new(10, 100);

        let pcb = create_test_pcb(110, 1000);
        assert!(store.insert(pcb));
        assert_eq!(store.total_count(), 1);
        assert_eq!(store.source_count(), 1);
    }

    #[test]
    fn test_beacon_store_multiple_sources() {
        let mut store = BeaconStore::new(10, 100);

        let pcb1 = create_test_pcb(110, 1000);
        let pcb2 = create_test_pcb(120, 1000);
        let pcb3 = create_test_pcb(110, 1001);

        assert!(store.insert(pcb1));
        assert!(store.insert(pcb2));
        assert!(store.insert(pcb3));

        assert_eq!(store.total_count(), 3);
        assert_eq!(store.source_count(), 2);
    }

    #[test]
    fn test_beacon_store_per_source_limit() {
        let mut store = BeaconStore::new(2, 100);

        let pcb1 = create_test_pcb(110, 1000);
        let pcb2 = create_test_pcb(110, 1001);
        let pcb3 = create_test_pcb(110, 1002);

        assert!(store.insert(pcb1));
        assert!(store.insert(pcb2));
        assert!(!store.insert(pcb3)); // Should be rejected

        assert_eq!(store.total_count(), 2);
    }

    #[test]
    fn test_beacon_store_get_by_source() {
        let mut store = BeaconStore::new(10, 100);

        let pcb1 = create_test_pcb(110, 1000);
        let pcb2 = create_test_pcb(120, 1000);

        store.insert(pcb1);
        store.insert(pcb2);

        let source = IsdAs::new(1, 110u64);
        let pcbs = store.get_by_source(&source);
        assert_eq!(pcbs.len(), 1);
    }

    #[test]
    fn test_beacon_store_remove_expired() {
        let mut store = BeaconStore::new(10, 100);

        // Create PCB with timestamp 1000
        let pcb = create_test_pcb(110, 1000);
        store.insert(pcb);

        // Remove expired at time 2000 (should not remove, as PCB exp is ~1000 + 24*60*60 seconds)
        let removed = store.remove_expired(2000, 0);
        assert_eq!(removed, 0);
        assert_eq!(store.total_count(), 1);

        // Remove expired at a much later time
        let removed = store.remove_expired(100000, 0);
        assert_eq!(removed, 1);
        assert_eq!(store.total_count(), 0);
    }

    #[test]
    fn test_path_database_creation() {
        let db: PathDatabase<SimplePrefix> = PathDatabase::new(100);
        assert_eq!(db.total_count(), 0);
    }

    #[test]
    fn test_path_database_register_up_segment() {
        let mut db = PathDatabase::new(100);

        let segment = create_test_segment(SegmentType::Up, 110, 120);
        assert!(db.register_segment(segment));
        assert_eq!(db.total_count(), 1);
        assert_eq!(db.get_all_up_segments().len(), 1);
    }

    #[test]
    fn test_path_database_register_down_segment() {
        let mut db = PathDatabase::new(100);

        let segment = create_test_segment(SegmentType::Down, 120, 110);
        assert!(db.register_segment(segment));
        assert_eq!(db.total_count(), 1);
        assert_eq!(db.get_all_down_segments().len(), 1);
    }

    #[test]
    fn test_path_database_register_core_segment() {
        let mut db = PathDatabase::new(100);

        let segment = create_test_segment(SegmentType::Core, 120, 130);
        assert!(db.register_segment(segment));
        assert_eq!(db.total_count(), 1);
        assert_eq!(db.get_all_core_segments().len(), 1);
    }

    #[test]
    fn test_path_database_lookup_up_segments() {
        let mut db = PathDatabase::new(100);

        // create_test_segment(Up, first_as, last_as) creates as_path [first, last]
        // Up-segment: source=last, destination=first
        // So create_test_segment(Up, 120, X) creates segment with dest=120
        let seg1 = create_test_segment(SegmentType::Up, 120, 110); // as_path=[120,110], dest=120
        let seg2 = create_test_segment(SegmentType::Up, 120, 111); // as_path=[120,111], dest=120
        let seg3 = create_test_segment(SegmentType::Up, 121, 112); // as_path=[121,112], dest=121

        db.register_segment(seg1);
        db.register_segment(seg2);
        db.register_segment(seg3);

        let dest = IsdAs::new(1, 120u64);
        let results = db.lookup_up_segments(&dest);
        assert_eq!(results.len(), 2); // Should find seg1 and seg2
    }

    #[test]
    fn test_path_database_lookup_down_segments() {
        let mut db = PathDatabase::new(100);

        // For Down segments, source() returns last AS, destination() returns first AS
        // So create_test_segment(Down, 110, 120) creates a segment with source=120, dest=110
        let seg1 = create_test_segment(SegmentType::Down, 110, 120);
        let seg2 = create_test_segment(SegmentType::Down, 111, 120);
        let seg3 = create_test_segment(SegmentType::Down, 112, 121);

        db.register_segment(seg1);
        db.register_segment(seg2);
        db.register_segment(seg3);

        let source = IsdAs::new(1, 120u64);
        let results = db.lookup_down_segments(&source);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_path_database_capacity_limit() {
        let mut db = PathDatabase::new(2);

        let seg1 = create_test_segment(SegmentType::Up, 110, 120);
        let seg2 = create_test_segment(SegmentType::Up, 111, 120);
        let seg3 = create_test_segment(SegmentType::Up, 112, 120);

        assert!(db.register_segment(seg1));
        assert!(db.register_segment(seg2));
        assert!(!db.register_segment(seg3)); // Should be rejected

        assert_eq!(db.get_all_up_segments().len(), 2);
    }

    #[test]
    fn test_path_database_remove_expired() {
        let mut db = PathDatabase::new(100);

        let segment = create_test_segment(SegmentType::Up, 110, 120);
        let expiration = segment.expiration;
        db.register_segment(segment);

        // Should not remove before expiration
        let removed = db.remove_expired(expiration - 100);
        assert_eq!(removed, 0);
        assert_eq!(db.total_count(), 1);

        // Should remove after expiration
        let removed = db.remove_expired(expiration + 1);
        assert_eq!(removed, 1);
        assert_eq!(db.total_count(), 0);
    }
}
