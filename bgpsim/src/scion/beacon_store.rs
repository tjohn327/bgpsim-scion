// Beacon Store for temporary PCB storage (OPTIMIZED FOR SCALE)
//
// The Beacon Store holds candidate PCBs before they are selected for
// propagation or registration.
//
// Optimizations:
// - Arc<Pcb> for zero-copy sharing
// - Indexed selection for O(limit) performance
// - Capacity limits with LRU eviction

use super::pcb::Pcb;
use super::types::IsdAs;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// Configuration for beacon store limits
#[derive(Debug, Clone)]
pub struct BeaconStoreConfig {
    /// Maximum PCBs per source AS
    pub max_pcbs_per_source: usize,

    /// Maximum total PCBs in store
    pub max_total_pcbs: usize,

    /// Maximum PCB age in seconds
    pub max_age_seconds: i64,
}

impl Default for BeaconStoreConfig {
    fn default() -> Self {
        Self {
            max_pcbs_per_source: 100,
            max_total_pcbs: 1000,
            max_age_seconds: 3600,  // 1 hour
        }
    }
}

/// Temporary storage for candidate PCBs (OPTIMIZED)
///
/// Maintains PCBs received from neighbors with:
/// - Indexed selection for fast querying
/// - Capacity limits to prevent memory exhaustion
#[derive(Debug, Clone)]
pub struct BeaconStore {
    // Primary storage: (source, segment_id) -> PCB
    beacons: HashMap<(IsdAs, u16), Arc<Pcb>>,

    // Index by path length for fast selection
    by_length: BTreeMap<usize, Vec<(IsdAs, u16)>>,

    // Index by source for per-source queries
    by_source: BTreeMap<IsdAs, Vec<(IsdAs, u16)>>,

    // Configuration limits
    config: BeaconStoreConfig,

    // Statistics
    total_inserted: usize,
    total_rejected: usize,
}

impl BeaconStore {
    /// Create a new empty beacon store with default configuration
    pub fn new() -> Self {
        Self::with_config(BeaconStoreConfig::default())
    }

    /// Create a beacon store with custom configuration
    pub fn with_config(config: BeaconStoreConfig) -> Self {
        Self {
            beacons: HashMap::new(),
            by_length: BTreeMap::new(),
            by_source: BTreeMap::new(),
            config,
            total_inserted: 0,
            total_rejected: 0,
        }
    }

    /// Insert a PCB into the store
    ///
    /// Returns true if inserted, false if rejected (duplicate, over limit, etc.)
    pub fn insert(&mut self, pcb: Arc<Pcb>) -> bool {
        if pcb.is_empty() {
            self.total_rejected += 1;
            return false;
        }

        let src = match pcb.src() {
            Some(s) => s,
            None => {
                self.total_rejected += 1;
                return false;
            }
        };

        let key = (src, pcb.segment_info.segment_id);

        // Check if replacing existing PCB
        if let Some(existing) = self.beacons.get(&key).cloned() {
            if pcb.segment_info.timestamp <= existing.segment_info.timestamp {
                self.total_rejected += 1;
                return false;  // Don't replace with older
            }
            // Remove old from indexes
            self.remove_from_indexes(&key, &existing);
        } else {
            // Check per-source limit before adding new
            let source_count = self.by_source.get(&src).map(|v| v.len()).unwrap_or(0);
            if source_count >= self.config.max_pcbs_per_source {
                // Evict worst PCB from this source
                self.evict_worst_from_source(src);
            }

            // Check total limit
            if self.beacons.len() >= self.config.max_total_pcbs {
                // Evict globally worst PCB
                self.evict_worst();
            }
        }

        // Insert into primary storage
        let pcb_len = pcb.len();
        self.beacons.insert(key, pcb);

        // Update indexes
        self.by_length.entry(pcb_len).or_default().push(key);
        self.by_source.entry(src).or_default().push(key);

        self.total_inserted += 1;
        true
    }

    /// Get a PCB by source and segment ID
    pub fn get(&self, src: IsdAs, segment_id: u16) -> Option<Arc<Pcb>> {
        self.beacons.get(&(src, segment_id)).cloned()
    }

    /// Get all PCBs in the store
    pub fn get_all(&self) -> impl Iterator<Item = Arc<Pcb>> + '_ {
        self.beacons.values().cloned()
    }

    /// Get PCBs from a specific source ISD-AS
    pub fn get_from_source(&self, src: IsdAs) -> Vec<Arc<Pcb>> {
        self.by_source.get(&src)
            .map(|keys| keys.iter().filter_map(|k| self.beacons.get(k).cloned()).collect())
            .unwrap_or_default()
    }

    /// Get the number of PCBs in the store
    pub fn len(&self) -> usize {
        self.beacons.len()
    }

    /// Check if the store is empty
    pub fn is_empty(&self) -> bool {
        self.beacons.is_empty()
    }

    /// Remove a specific PCB
    pub fn remove(&mut self, src: IsdAs, segment_id: u16) -> Option<Arc<Pcb>> {
        let key = (src, segment_id);
        if let Some(pcb) = self.beacons.remove(&key) {
            self.remove_from_indexes(&key, &pcb);
            Some(pcb)
        } else {
            None
        }
    }

    /// Clear all PCBs from the store
    pub fn clear(&mut self) {
        self.beacons.clear();
        self.by_length.clear();
        self.by_source.clear();
    }

    /// Remove PCBs older than given timestamp
    pub fn remove_expired(&mut self, min_timestamp: i64) -> usize {
        let to_remove: Vec<_> = self.beacons.iter()
            .filter(|(_, pcb)| pcb.segment_info.timestamp < min_timestamp)
            .map(|(key, _)| *key)
            .collect();

        for key in &to_remove {
            if let Some(pcb) = self.beacons.remove(key) {
                self.remove_from_indexes(key, &pcb);
            }
        }

        to_remove.len()
    }

    /// Select best N PCBs (shortest paths) - O(limit) performance
    pub fn select_best(&self, limit: usize) -> Vec<Arc<Pcb>> {
        self.by_length.values()
            .flatten()
            .filter_map(|key| self.beacons.get(key).cloned())
            .take(limit)
            .collect()
    }

    /// Select PCBs by shortest path length
    pub fn select_by_length(&self, limit: usize) -> Vec<Arc<Pcb>> {
        self.select_best(limit)  // Same as select_best
    }

    /// Select PCBs that don't contain a specific ISD-AS
    pub fn select_excluding(&self, exclude: IsdAs) -> Vec<Arc<Pcb>> {
        self.beacons.values()
            .filter(|pcb| !pcb.contains(exclude))
            .cloned()
            .collect()
    }

    /// Select diverse PCBs (maximize AS coverage)
    pub fn select_diverse(&self, limit: usize) -> Vec<Arc<Pcb>> {
        let mut selected = Vec::new();
        let mut covered_ases = HashSet::new();

        // Greedy selection: pick PCB covering most new ASes
        let mut candidates: Vec<_> = self.beacons.values().cloned().collect();

        while selected.len() < limit && !candidates.is_empty() {
            // Find PCB with most new ASes
            let (best_idx, _) = candidates.iter()
                .enumerate()
                .map(|(idx, pcb)| {
                    let new_ases = pcb.as_path().into_iter()
                        .filter(|as_| !covered_ases.contains(as_))
                        .count();
                    (idx, new_ases)
                })
                .max_by_key(|(_, count)| *count)
                .unwrap_or((0, 0));

            let best = candidates.swap_remove(best_idx);
            covered_ases.extend(best.as_path());
            selected.push(best);
        }

        selected
    }

    /// Group PCBs by their source ISD-AS
    pub fn group_by_source(&self) -> BTreeMap<IsdAs, Vec<Arc<Pcb>>> {
        let mut grouped = BTreeMap::new();

        for (src, keys) in &self.by_source {
            let pcbs = keys.iter()
                .filter_map(|k| self.beacons.get(k).cloned())
                .collect();
            grouped.insert(*src, pcbs);
        }

        grouped
    }

    /// Enforce capacity limits (call periodically)
    pub fn enforce_limits(&mut self) {
        // Remove expired PCBs
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.remove_expired(now - self.config.max_age_seconds);

        // Enforce total limit
        while self.beacons.len() > self.config.max_total_pcbs {
            self.evict_worst();
        }
    }

    /// Get statistics about the store
    pub fn stats(&self) -> BeaconStoreStats {
        BeaconStoreStats {
            total_pcbs: self.beacons.len(),
            unique_sources: self.by_source.len(),
            total_inserted: self.total_inserted,
            total_rejected: self.total_rejected,
            avg_path_length: self.average_path_length(),
        }
    }

    // Private helper methods

    fn remove_from_indexes(&mut self, key: &(IsdAs, u16), pcb: &Pcb) {
        let (src, _) = key;

        // Remove from by_length
        if let Some(keys) = self.by_length.get_mut(&pcb.len()) {
            keys.retain(|k| k != key);
            if keys.is_empty() {
                self.by_length.remove(&pcb.len());
            }
        }

        // Remove from by_source
        if let Some(keys) = self.by_source.get_mut(src) {
            keys.retain(|k| k != key);
            if keys.is_empty() {
                self.by_source.remove(src);
            }
        }
    }

    fn evict_worst(&mut self) {
        // Evict longest PCB (worst for path selection)
        if let Some((_max_len, keys)) = self.by_length.iter().next_back() {
            if let Some(&key) = keys.first() {
                if let Some(pcb) = self.beacons.remove(&key) {
                    self.remove_from_indexes(&key, &pcb);
                }
            }
        }
    }

    fn evict_worst_from_source(&mut self, src: IsdAs) {
        // Evict longest PCB from this source
        if let Some(keys) = self.by_source.get(&src) {
            // Find longest PCB from this source
            let longest_key = keys.iter()
                .filter_map(|k| self.beacons.get(k).map(|pcb| (k, pcb.len())))
                .max_by_key(|(_, len)| *len)
                .map(|(k, _)| *k);

            if let Some(key) = longest_key {
                if let Some(pcb) = self.beacons.remove(&key) {
                    self.remove_from_indexes(&key, &pcb);
                }
            }
        }
    }

    fn average_path_length(&self) -> f64 {
        if self.beacons.is_empty() {
            return 0.0;
        }

        let total: usize = self.beacons.values().map(|pcb| pcb.len()).sum();
        total as f64 / self.beacons.len() as f64
    }
}

impl Default for BeaconStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about a beacon store
#[derive(Debug, Clone, Copy)]
pub struct BeaconStoreStats {
    pub total_pcbs: usize,
    pub unique_sources: usize,
    pub total_inserted: usize,
    pub total_rejected: usize,
    pub avg_path_length: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scion::pcb::{AsEntry, HopEntry, SegmentInfo};
    use crate::scion::types::InterfaceId;

    fn create_test_pcb(isd_as: IsdAs, segment_id: u16, timestamp: i64, hops: usize) -> Arc<Pcb> {
        let mut pcb = Pcb::with_segment_info(SegmentInfo::with_values(timestamp, segment_id));

        for i in 0..hops {
            let entry = AsEntry::new(
                IsdAs::new(isd_as.isd.as_u16(), isd_as.asn.0 + i as u32),
                if i < hops - 1 {
                    Some(IsdAs::new(isd_as.isd.as_u16(), isd_as.asn.0 + (i + 1) as u32))
                } else {
                    None
                },
                HopEntry::new(
                    InterfaceId::new(i as u16),
                    if i < hops - 1 {
                        Some(InterfaceId::new((i + 1) as u16))
                    } else {
                        None
                    },
                ),
            );
            pcb.extend(entry);
        }

        Arc::new(pcb)
    }

    #[test]
    fn test_beacon_store_creation() {
        let store = BeaconStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut store = BeaconStore::new();
        let pcb = create_test_pcb(IsdAs::new(1, 100), 42, 1000, 3);

        assert!(store.insert(pcb.clone()));
        assert_eq!(store.len(), 1);

        let retrieved = store.get(IsdAs::new(1, 100), 42);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().segment_info.segment_id, 42);
    }

    #[test]
    fn test_insert_empty_pcb() {
        let mut store = BeaconStore::new();
        let empty_pcb = Arc::new(Pcb::new(IsdAs::new(1, 100)));

        assert!(!store.insert(empty_pcb));
        assert!(store.is_empty());
    }

    #[test]
    fn test_replace_with_newer() {
        let mut store = BeaconStore::new();

        let old_pcb = create_test_pcb(IsdAs::new(1, 100), 42, 1000, 2);
        let new_pcb = create_test_pcb(IsdAs::new(1, 100), 42, 2000, 2);

        store.insert(old_pcb);
        assert_eq!(store.len(), 1);

        assert!(store.insert(new_pcb.clone()));
        assert_eq!(store.len(), 1);

        let retrieved = store.get(IsdAs::new(1, 100), 42).unwrap();
        assert_eq!(retrieved.segment_info.timestamp, 2000);
    }

    #[test]
    fn test_reject_older() {
        let mut store = BeaconStore::new();

        let new_pcb = create_test_pcb(IsdAs::new(1, 100), 42, 2000, 2);
        let old_pcb = create_test_pcb(IsdAs::new(1, 100), 42, 1000, 2);

        store.insert(new_pcb);
        assert_eq!(store.len(), 1);

        assert!(!store.insert(old_pcb));
        assert_eq!(store.len(), 1);

        let retrieved = store.get(IsdAs::new(1, 100), 42).unwrap();
        assert_eq!(retrieved.segment_info.timestamp, 2000);
    }

    #[test]
    fn test_different_segment_ids() {
        let mut store = BeaconStore::new();

        // Create two PCBs with same AS path but different segment IDs
        // Both should be accepted since they represent different beaconing instances
        let pcb1 = create_test_pcb(IsdAs::new(1, 100), 1, 1000, 3);
        let pcb2 = create_test_pcb(IsdAs::new(1, 100), 2, 1000, 3);

        assert!(store.insert(pcb1));
        assert!(store.insert(pcb2));  // Accepted - different segment ID
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_multiple_pcbs() {
        let mut store = BeaconStore::new();

        let pcb1 = create_test_pcb(IsdAs::new(1, 100), 1, 1000, 2);
        let pcb2 = create_test_pcb(IsdAs::new(1, 100), 2, 1000, 3);  // Different length
        let pcb3 = create_test_pcb(IsdAs::new(1, 101), 1, 1000, 2);  // Different source

        store.insert(pcb1);
        store.insert(pcb2);
        store.insert(pcb3);

        assert_eq!(store.len(), 3);
    }

    #[test]
    fn test_get_from_source() {
        let mut store = BeaconStore::new();

        let pcb1 = create_test_pcb(IsdAs::new(1, 100), 1, 1000, 2);
        let pcb2 = create_test_pcb(IsdAs::new(1, 100), 2, 1000, 3);
        let pcb3 = create_test_pcb(IsdAs::new(1, 101), 1, 1000, 2);

        store.insert(pcb1);
        store.insert(pcb2);
        store.insert(pcb3);

        let from_100 = store.get_from_source(IsdAs::new(1, 100));
        assert_eq!(from_100.len(), 2);

        let from_101 = store.get_from_source(IsdAs::new(1, 101));
        assert_eq!(from_101.len(), 1);
    }

    #[test]
    fn test_select_by_length() {
        let mut store = BeaconStore::new();

        // Insert PCBs with different lengths
        store.insert(create_test_pcb(IsdAs::new(1, 100), 1, 1000, 2));
        store.insert(create_test_pcb(IsdAs::new(1, 101), 2, 1000, 4));
        store.insert(create_test_pcb(IsdAs::new(1, 102), 3, 1000, 6));

        let best = store.select_by_length(2);
        assert_eq!(best.len(), 2);
        assert_eq!(best[0].len(), 2);  // Shortest first
        assert_eq!(best[1].len(), 4);
    }

    #[test]
    fn test_capacity_limit() {
        let config = BeaconStoreConfig {
            max_total_pcbs: 5,
            ..Default::default()
        };
        let mut store = BeaconStore::with_config(config);

        // Insert 10 PCBs
        for i in 0..10 {
            let pcb = create_test_pcb(IsdAs::new(1, 100 + i), i as u16, 1000, ((i % 3) + 2) as usize);
            store.insert(pcb);
        }

        // Should only keep 5
        assert!(store.len() <= 5);
    }

    #[test]
    fn test_per_source_limit() {
        let config = BeaconStoreConfig {
            max_pcbs_per_source: 3,
            ..Default::default()
        };
        let mut store = BeaconStore::with_config(config);

        // Insert 10 PCBs from same source
        for i in 0..10 {
            let pcb = create_test_pcb(IsdAs::new(1, 100), i as u16, 1000 + i as i64, ((i % 3) + 2) as usize);
            store.insert(pcb);
        }

        // Should only keep 3 from this source
        assert!(store.get_from_source(IsdAs::new(1, 100)).len() <= 3);
    }

    #[test]
    fn test_select_diverse() {
        let mut store = BeaconStore::new();

        // PCB1: AS100 -> AS101 -> AS102
        store.insert(create_test_pcb(IsdAs::new(1, 100), 1, 1000, 3));
        // PCB2: AS110 -> AS111 -> AS112
        store.insert(create_test_pcb(IsdAs::new(1, 110), 2, 1000, 3));
        // PCB3: AS100 -> AS101 -> AS103 (overlaps with PCB1)
        store.insert(create_test_pcb(IsdAs::new(1, 100), 3, 1001, 3));

        let diverse = store.select_diverse(2);
        assert_eq!(diverse.len(), 2);

        // Should select PCB1 and PCB2 (maximum AS coverage)
        let sources: Vec<_> = diverse.iter().map(|p| p.src().unwrap()).collect();
        assert!(sources.contains(&IsdAs::new(1, 100)));
        assert!(sources.contains(&IsdAs::new(1, 110)));
    }

    #[test]
    fn test_stats() {
        let mut store = BeaconStore::new();

        store.insert(create_test_pcb(IsdAs::new(1, 100), 1, 1000, 2));
        store.insert(create_test_pcb(IsdAs::new(1, 100), 2, 1000, 4));
        store.insert(create_test_pcb(IsdAs::new(1, 101), 1, 1000, 3));

        let stats = store.stats();
        assert_eq!(stats.total_pcbs, 3);
        assert_eq!(stats.unique_sources, 2);
        assert_eq!(stats.total_inserted, 3);
        assert_eq!(stats.avg_path_length, 3.0);  // (2 + 4 + 3) / 3
    }

    #[test]
    fn test_remove_expired() {
        let mut store = BeaconStore::new();

        store.insert(create_test_pcb(IsdAs::new(1, 100), 1, 1000, 2));
        store.insert(create_test_pcb(IsdAs::new(1, 100), 2, 1500, 2));
        store.insert(create_test_pcb(IsdAs::new(1, 101), 1, 3000, 2));

        assert_eq!(store.len(), 3);

        let removed = store.remove_expired(2000);
        assert_eq!(removed, 2);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut store = BeaconStore::new();

        for i in 0..5 {
            store.insert(create_test_pcb(IsdAs::new(1, 100 + i), i as u16, 1000, 2));
        }

        assert_eq!(store.len(), 5);

        store.clear();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }
}
