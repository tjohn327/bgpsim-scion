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

//! Path selection policies for choosing among multiple forwarding paths.
//!
//! This module provides traits and implementations for selecting the best forwarding
//! paths from a set of available options. Different policies optimize for different
//! metrics such as path length, MTU, or custom scoring functions.

use crate::types::Prefix;

use super::ForwardingPath;

/// Trait for selecting forwarding paths based on policy.
///
/// Implementations of this trait define different strategies for ranking and
/// selecting paths, such as preferring shortest paths, highest MTU, or
/// application-specific criteria.
pub trait PathSelectionPolicy<P: Prefix> {
    /// Select the best paths from the available set.
    ///
    /// # Arguments
    /// * `paths` - Available paths to choose from
    /// * `max_count` - Maximum number of paths to select (for multipath)
    ///
    /// # Returns
    /// Indices of selected paths from the input vector, ordered by preference
    fn select_paths(&self, paths: &[&ForwardingPath<P>], max_count: usize) -> Vec<usize>;
}

/// Select paths based on shortest AS-level path length.
///
/// This policy prefers paths with the fewest AS hops. When multiple paths
/// have the same length, they are selected in the order they appear.
#[derive(Debug, Clone, Default)]
pub struct ShortestPathPolicy;

impl<P: Prefix> PathSelectionPolicy<P> for ShortestPathPolicy {
    fn select_paths(&self, paths: &[&ForwardingPath<P>], max_count: usize) -> Vec<usize> {
        if paths.is_empty() || max_count == 0 {
            return Vec::new();
        }

        // Create indices with path lengths
        let mut indexed_paths: Vec<(usize, usize)> = paths
            .iter()
            .enumerate()
            .map(|(idx, path)| (idx, path.length()))
            .collect();

        // Sort by path length (ascending)
        indexed_paths.sort_by_key(|(_, len)| *len);

        // Take the shortest paths up to max_count
        indexed_paths
            .into_iter()
            .take(max_count)
            .map(|(idx, _)| idx)
            .collect()
    }
}

/// Select paths based on highest MTU.
///
/// This policy prefers paths with the highest MTU value, which allows for
/// larger packets without fragmentation. When multiple paths have the same
/// MTU, they are selected in the order they appear.
#[derive(Debug, Clone, Default)]
pub struct HighestMtuPolicy;

impl<P: Prefix> PathSelectionPolicy<P> for HighestMtuPolicy {
    fn select_paths(&self, paths: &[&ForwardingPath<P>], max_count: usize) -> Vec<usize> {
        if paths.is_empty() || max_count == 0 {
            return Vec::new();
        }

        // Create indices with MTU values
        let mut indexed_paths: Vec<(usize, u16)> = paths
            .iter()
            .enumerate()
            .map(|(idx, path)| (idx, path.mtu))
            .collect();

        // Sort by MTU (descending)
        indexed_paths.sort_by_key(|(_, mtu)| std::cmp::Reverse(*mtu));

        // Take the highest MTU paths up to max_count
        indexed_paths
            .into_iter()
            .take(max_count)
            .map(|(idx, _)| idx)
            .collect()
    }
}

/// Select the first N paths without any ranking.
///
/// This is a simple policy that returns paths in the order they are provided,
/// useful for testing or when all paths are considered equally good.
#[derive(Debug, Clone, Default)]
pub struct FirstNPolicy;

impl<P: Prefix> PathSelectionPolicy<P> for FirstNPolicy {
    fn select_paths(&self, paths: &[&ForwardingPath<P>], max_count: usize) -> Vec<usize> {
        if paths.is_empty() || max_count == 0 {
            return Vec::new();
        }

        (0..paths.len().min(max_count)).collect()
    }
}

/// Select all available paths.
///
/// This policy returns all paths without filtering, useful when the application
/// wants to handle path selection itself or use all available paths.
#[derive(Debug, Clone, Default)]
pub struct AllPathsPolicy;

impl<P: Prefix> PathSelectionPolicy<P> for AllPathsPolicy {
    fn select_paths(&self, paths: &[&ForwardingPath<P>], _max_count: usize) -> Vec<usize> {
        (0..paths.len()).collect()
    }
}

/// Composite policy that combines multiple selection strategies.
///
/// This policy applies multiple sub-policies in sequence, using the first
/// policy's ranking as the primary criterion, the second as a tiebreaker, etc.
pub struct CompositePolicy<P: Prefix> {
    policies: Vec<Box<dyn PathSelectionPolicy<P>>>,
}

impl<P: Prefix> std::fmt::Debug for CompositePolicy<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositePolicy")
            .field("policies", &format!("{} policies", self.policies.len()))
            .finish()
    }
}

impl<P: Prefix> CompositePolicy<P> {
    /// Create a new composite policy from a list of sub-policies.
    ///
    /// Policies are applied in order: the first policy provides the primary
    /// ranking, subsequent policies break ties.
    pub fn new(policies: Vec<Box<dyn PathSelectionPolicy<P>>>) -> Self {
        CompositePolicy { policies }
    }
}

impl<P: Prefix> PathSelectionPolicy<P> for CompositePolicy<P> {
    fn select_paths(&self, paths: &[&ForwardingPath<P>], max_count: usize) -> Vec<usize> {
        if paths.is_empty() || max_count == 0 || self.policies.is_empty() {
            return Vec::new();
        }

        // Apply first policy
        let mut selected = self.policies[0].select_paths(paths, paths.len());

        // Apply subsequent policies to refine the ranking
        for policy in &self.policies[1..] {
            let refined_paths: Vec<&ForwardingPath<P>> =
                selected.iter().map(|&idx| paths[idx]).collect();
            let refined_indices = policy.select_paths(&refined_paths, refined_paths.len());

            // Map refined indices back to original indices
            selected = refined_indices
                .into_iter()
                .map(|refined_idx| selected[refined_idx])
                .collect();
        }

        // Return up to max_count paths
        selected.into_iter().take(max_count).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SimplePrefix;
    use crate::scion::types::IsdAs;

    fn create_test_path(as_count: usize, mtu: u16) -> ForwardingPath<SimplePrefix> {
        // Create a simple path with the specified number of ASes
        let mut as_path = Vec::new();
        for i in 0..as_count {
            as_path.push(IsdAs::new(1, 110 + i as u64));
        }

        ForwardingPath {
            up_segment: None,
            core_segment: None,
            down_segment: None,
            peering_shortcut: None,
            as_path,
            mtu,
        }
    }

    #[test]
    fn test_shortest_path_policy() {
        let paths = vec![
            create_test_path(5, 1500),
            create_test_path(3, 1500),
            create_test_path(4, 1500),
            create_test_path(2, 1500),
        ];
        let path_refs: Vec<&ForwardingPath<SimplePrefix>> = paths.iter().collect();

        let policy = ShortestPathPolicy;
        let selected = policy.select_paths(&path_refs, 2);

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0], 3); // Path with 2 ASes
        assert_eq!(selected[1], 1); // Path with 3 ASes
    }

    #[test]
    fn test_highest_mtu_policy() {
        let paths = vec![
            create_test_path(3, 1500),
            create_test_path(3, 9000),
            create_test_path(3, 1400),
            create_test_path(3, 4470),
        ];
        let path_refs: Vec<&ForwardingPath<SimplePrefix>> = paths.iter().collect();

        let policy = HighestMtuPolicy;
        let selected = policy.select_paths(&path_refs, 2);

        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0], 1); // MTU 9000
        assert_eq!(selected[1], 3); // MTU 4470
    }

    #[test]
    fn test_first_n_policy() {
        let paths = vec![
            create_test_path(5, 1500),
            create_test_path(3, 1400),
            create_test_path(4, 9000),
        ];
        let path_refs: Vec<&ForwardingPath<SimplePrefix>> = paths.iter().collect();

        let policy = FirstNPolicy;
        let selected = policy.select_paths(&path_refs, 2);

        assert_eq!(selected.len(), 2);
        assert_eq!(selected, vec![0, 1]);
    }

    #[test]
    fn test_all_paths_policy() {
        let paths = vec![
            create_test_path(5, 1500),
            create_test_path(3, 1400),
            create_test_path(4, 9000),
        ];
        let path_refs: Vec<&ForwardingPath<SimplePrefix>> = paths.iter().collect();

        let policy = AllPathsPolicy;
        let selected = policy.select_paths(&path_refs, 1); // max_count ignored

        assert_eq!(selected.len(), 3);
        assert_eq!(selected, vec![0, 1, 2]);
    }

    #[test]
    fn test_selection_with_empty_paths() {
        let paths: Vec<ForwardingPath<SimplePrefix>> = vec![];
        let path_refs: Vec<&ForwardingPath<SimplePrefix>> = paths.iter().collect();

        let policy = ShortestPathPolicy;
        let selected = policy.select_paths(&path_refs, 5);

        assert_eq!(selected.len(), 0);
    }

    #[test]
    fn test_selection_with_zero_max_count() {
        let paths = vec![create_test_path(3, 1500)];
        let path_refs: Vec<&ForwardingPath<SimplePrefix>> = paths.iter().collect();

        let policy = ShortestPathPolicy;
        let selected = policy.select_paths(&path_refs, 0);

        assert_eq!(selected.len(), 0);
    }

    #[test]
    fn test_shortest_path_with_ties() {
        // When paths have the same length, should preserve order
        let paths = vec![
            create_test_path(3, 1500), // idx 0
            create_test_path(3, 1400), // idx 1
            create_test_path(3, 9000), // idx 2
        ];
        let path_refs: Vec<&ForwardingPath<SimplePrefix>> = paths.iter().collect();

        let policy = ShortestPathPolicy;
        let selected = policy.select_paths(&path_refs, 3);

        assert_eq!(selected.len(), 3);
        // All have same length, should be in original order
        assert_eq!(selected, vec![0, 1, 2]);
    }
}
