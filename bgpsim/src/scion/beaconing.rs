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

//! SCION beaconing implementation for path discovery.
//!
//! This module implements the core beaconing process where ASes create and propagate
//! Path Construction Beacons (PCBs) to discover paths through the network.

use crate::types::Prefix;

use super::{
    pcb::{AsEntry, HopEntry, HopField, Pcb, PeerEntry, SegmentFlags, SegmentInfo},
    types::{InterfaceId, IsdAs},
};

/// PCB selection policy trait.
///
/// Defines how ASes select which PCBs to propagate to neighbors.
pub trait SelectionPolicy<P: Prefix> {
    /// Select PCBs to propagate from the available set.
    ///
    /// # Arguments
    /// * `pcbs` - Available PCBs to choose from
    /// * `max_count` - Maximum number of PCBs to select
    ///
    /// # Returns
    /// Indices of selected PCBs from the input vector
    fn select_pcbs(&self, pcbs: &[&Pcb<P>], max_count: usize) -> Vec<usize>;
}

/// Simple selection policy that prefers shorter paths.
///
/// Selects up to `max_count` PCBs with the shortest AS path length.
#[derive(Debug, Clone, PartialEq)]
pub struct SimpleSelectionPolicy;

impl<P: Prefix> SelectionPolicy<P> for SimpleSelectionPolicy {
    fn select_pcbs(&self, pcbs: &[&Pcb<P>], max_count: usize) -> Vec<usize> {
        if pcbs.is_empty() {
            return Vec::new();
        }

        // Create indexed pairs (index, path_length)
        let mut indexed: Vec<(usize, usize)> = pcbs
            .iter()
            .enumerate()
            .map(|(idx, pcb)| (idx, pcb.path_length()))
            .collect();

        // Sort by path length (shorter paths first)
        indexed.sort_by_key(|(_, len)| *len);

        // Take up to max_count
        indexed.iter().take(max_count).map(|(idx, _)| *idx).collect()
    }
}

/// Create an initial PCB for a core AS.
///
/// This generates a new PCB that a core AS will propagate to its core neighbors.
///
/// # Arguments
/// * `isd_as` - The ISD-AS of the originating core AS
/// * `timestamp` - Current simulation timestamp
/// * `egress_if` - Egress interface to the neighbor
/// * `mtu` - MTU for this hop
///
/// # Returns
/// A new PCB with a single AS entry for the originating core AS
pub fn create_initial_pcb<P: Prefix>(
    isd_as: IsdAs,
    timestamp: u32,
    egress_if: InterfaceId,
    mtu: u16,
) -> Pcb<P> {
    // Generate a segment ID (in a real implementation, this would be random)
    let segment_id = timestamp as u64;

    let info = SegmentInfo {
        timestamp,
        segment_id,
        flags: SegmentFlags { reserved: 0 },
    };

    let mut pcb = Pcb::new(info);

    // Create hop field for the initial AS
    // Ingress is unspecified for the originating AS
    let hop_field = HopField {
        ingress: InterfaceId::UNSPECIFIED,
        egress: egress_if,
        exp_time: 63, // Maximum expiration time
        mac: generate_mac(isd_as, egress_if, timestamp), // Simplified MAC
    };

    let hop_entry = HopEntry {
        hop_field,
        ingress_mtu: mtu,
    };

    let as_entry = AsEntry::new(isd_as, hop_entry);
    pcb.add_as_entry(as_entry);

    pcb
}

/// Extend a PCB by adding this AS's entry.
///
/// # Arguments
/// * `pcb` - The PCB to extend
/// * `isd_as` - This AS's ISD-AS
/// * `ingress_if` - Ingress interface where the PCB arrived
/// * `egress_if` - Egress interface to the next hop
/// * `ingress_mtu` - MTU of the ingress interface
/// * `timestamp` - Current simulation timestamp
///
/// # Returns
/// Extended PCB (the original is consumed)
pub fn extend_pcb<P: Prefix>(
    mut pcb: Pcb<P>,
    isd_as: IsdAs,
    ingress_if: InterfaceId,
    egress_if: InterfaceId,
    ingress_mtu: u16,
    timestamp: u32,
) -> Pcb<P> {
    let hop_field = HopField {
        ingress: ingress_if,
        egress: egress_if,
        exp_time: 63, // Maximum expiration time
        mac: generate_mac(isd_as, ingress_if, timestamp),
    };

    let hop_entry = HopEntry {
        hop_field,
        ingress_mtu,
    };

    let as_entry = AsEntry::new(isd_as, hop_entry);
    pcb.add_as_entry(as_entry);

    pcb
}

/// Generate a simplified MAC for a hop field.
///
/// In a real SCION implementation, this would be a cryptographic MAC.
/// For simulation purposes, we use a simple hash-like value.
fn generate_mac(isd_as: IsdAs, interface: InterfaceId, timestamp: u32) -> u64 {
    // Simple hash combining all inputs
    let mut mac = 0u64;
    mac ^= isd_as.isd.0 as u64;
    mac ^= isd_as.asn.as_u64();
    mac ^= (interface.0 as u64) << 16;
    mac ^= (timestamp as u64) << 32;
    mac
}

/// Validate a received PCB.
///
/// Checks that the PCB is valid for processing:
/// - Timestamp is reasonable
/// - No loops (for core beaconing)
/// - Not expired
/// - Hop fields are valid
///
/// # Arguments
/// * `pcb` - The PCB to validate
/// * `current_time` - Current simulation timestamp
/// * `is_core` - Whether this AS is a core AS (affects loop detection)
///
/// # Returns
/// `Ok(())` if valid, `Err(String)` with reason if invalid
pub fn validate_pcb<P: Prefix>(
    pcb: &Pcb<P>,
    current_time: u32,
    is_core: bool,
) -> Result<(), String> {
    // Use the PCB's built-in validation
    pcb.validate(current_time, 300) // 5 minute tolerance
        .map_err(|e| format!("{:?}", e))?;

    // For core ASes, check for loops (duplicate AS entries)
    if is_core && pcb.has_loop() {
        return Err("PCB contains a loop".to_string());
    }

    Ok(())
}

/// Extend a PCB with peering information.
///
/// When an AS has peering links, it can add peer entries to the PCB
/// so that downstream ASes are aware of potential path shortcuts.
///
/// # Arguments
/// * `pcb` - The PCB to extend (will be modified in place after cloning)
/// * `peering_links` - Vector of peering link information (peer ISD-AS, interface IDs, MTU)
/// * `timestamp` - Current simulation timestamp
///
/// # Returns
/// The PCB with peer entries added to the last AS entry
pub fn add_peering_entries<P: Prefix>(
    mut pcb: Pcb<P>,
    peering_links: Vec<(IsdAs, InterfaceId, InterfaceId, u16)>, // (peer_isd_as, local_if, peer_if, mtu)
    timestamp: u32,
) -> Pcb<P> {
    if let Some(last_entry) = pcb.as_entries.last_mut() {
        for (peer_isd_as, local_if, peer_if, mtu) in peering_links {
            // Create hop field for the peering link
            let hop_field = HopField {
                ingress: local_if,
                egress: peer_if,
                exp_time: 63,
                mac: generate_mac(peer_isd_as, local_if, timestamp),
            };

            let peer_entry = PeerEntry::new(peer_isd_as, peer_if, mtu, hop_field);
            last_entry.add_peer_entry(peer_entry);
        }
    }

    pcb
}

/// Select PCBs for propagation to children or peers.
///
/// Uses a selection policy to choose which PCBs to propagate downstream.
/// This helps manage the number of beacons in the network and focus on
/// diverse/high-quality paths.
///
/// # Arguments
/// * `pcbs` - Available PCBs from beacon store
/// * `policy` - Selection policy to use
/// * `max_count` - Maximum number of PCBs to select
///
/// # Returns
/// Selected PCBs to propagate
pub fn select_for_propagation<P: Prefix>(
    pcbs: &[Pcb<P>],
    policy: &dyn SelectionPolicy<P>,
    max_count: usize,
) -> Vec<Pcb<P>> {
    if pcbs.is_empty() {
        return Vec::new();
    }

    // Create references for the policy
    let pcb_refs: Vec<&Pcb<P>> = pcbs.iter().collect();

    // Use the policy to select indices
    let selected_indices = policy.select_pcbs(&pcb_refs, max_count);

    // Return clones of the selected PCBs
    selected_indices.iter().map(|&idx| pcbs[idx].clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SimplePrefix;

    #[test]
    fn test_create_initial_pcb() {
        let isd_as = IsdAs::new(1, 110u64);
        let pcb: Pcb<SimplePrefix> =
            create_initial_pcb(isd_as, 1000, InterfaceId(1), 1500);

        assert_eq!(pcb.path_length(), 1);
        assert_eq!(pcb.get_origin(), Some(isd_as));
        assert_eq!(pcb.get_last_as(), Some(isd_as));
    }

    #[test]
    fn test_extend_pcb() {
        let isd_as1 = IsdAs::new(1, 110u64);
        let isd_as2 = IsdAs::new(1, 120u64);

        let pcb: Pcb<SimplePrefix> =
            create_initial_pcb(isd_as1, 1000, InterfaceId(1), 1500);

        let extended = extend_pcb(
            pcb,
            isd_as2,
            InterfaceId(2),
            InterfaceId(3),
            1500,
            1000,
        );

        assert_eq!(extended.path_length(), 2);
        assert_eq!(extended.get_origin(), Some(isd_as1));
        assert_eq!(extended.get_last_as(), Some(isd_as2));
    }

    #[test]
    fn test_validate_pcb_success() {
        let isd_as = IsdAs::new(1, 110u64);
        let pcb: Pcb<SimplePrefix> =
            create_initial_pcb(isd_as, 1000, InterfaceId(1), 1500);

        let result = validate_pcb(&pcb, 1100, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_pcb_expired() {
        let isd_as = IsdAs::new(1, 110u64);
        let pcb: Pcb<SimplePrefix> =
            create_initial_pcb(isd_as, 1000, InterfaceId(1), 1500);

        // PCB expires after ~24 hours, so this should fail
        let result = validate_pcb(&pcb, 1000 + 100000, true);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_pcb_with_loop() {
        let isd_as = IsdAs::new(1, 110u64);
        let mut pcb: Pcb<SimplePrefix> =
            create_initial_pcb(isd_as, 1000, InterfaceId(1), 1500);

        // Extend with same AS (creates loop)
        let hop_field = HopField {
            ingress: InterfaceId(2),
            egress: InterfaceId(3),
            exp_time: 63,
            mac: 0,
        };
        let hop_entry = HopEntry {
            hop_field,
            ingress_mtu: 1500,
        };
        pcb.add_as_entry(AsEntry::new(isd_as, hop_entry));

        let result = validate_pcb(&pcb, 1100, true);
        assert!(result.is_err());
        let err = result.unwrap_err();
        // The error should mention loop
        assert!(err.to_lowercase().contains("loop") || err.contains("duplicate"));
    }

    #[test]
    fn test_simple_selection_policy() {
        let policy = SimpleSelectionPolicy;

        // Create PCBs with different path lengths
        let pcb1: Pcb<SimplePrefix> =
            create_initial_pcb(IsdAs::new(1, 110u64), 1000, InterfaceId(1), 1500);

        let pcb2 = extend_pcb(
            create_initial_pcb(IsdAs::new(1, 120u64), 1000, InterfaceId(1), 1500),
            IsdAs::new(1, 130u64),
            InterfaceId(2),
            InterfaceId(3),
            1500,
            1000,
        );

        let pcb3 = extend_pcb(
            extend_pcb(
                create_initial_pcb(IsdAs::new(1, 140u64), 1000, InterfaceId(1), 1500),
                IsdAs::new(1, 150u64),
                InterfaceId(2),
                InterfaceId(3),
                1500,
                1000,
            ),
            IsdAs::new(1, 160u64),
            InterfaceId(4),
            InterfaceId(5),
            1500,
            1000,
        );

        let pcbs = vec![&pcb1, &pcb2, &pcb3];

        // Select 2 PCBs - should pick the two shortest
        let selected = policy.select_pcbs(&pcbs, 2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0], 0); // pcb1 has length 1
        assert_eq!(selected[1], 1); // pcb2 has length 2
    }

    #[test]
    fn test_simple_selection_policy_empty() {
        let policy = SimpleSelectionPolicy;
        let pcbs: Vec<&Pcb<SimplePrefix>> = vec![];
        let selected = policy.select_pcbs(&pcbs, 5);
        assert_eq!(selected.len(), 0);
    }

    #[test]
    fn test_simple_selection_policy_limit() {
        let policy = SimpleSelectionPolicy;

        let pcb1: Pcb<SimplePrefix> =
            create_initial_pcb(IsdAs::new(1, 110u64), 1000, InterfaceId(1), 1500);
        let pcb2: Pcb<SimplePrefix> =
            create_initial_pcb(IsdAs::new(1, 120u64), 1000, InterfaceId(1), 1500);

        let pcbs = vec![&pcb1, &pcb2];

        // Request more than available
        let selected = policy.select_pcbs(&pcbs, 10);
        assert_eq!(selected.len(), 2); // Should return all available
    }

    #[test]
    fn test_add_peering_entries() {
        let isd_as1 = IsdAs::new(1, 110u64);
        let isd_as2 = IsdAs::new(1, 120u64);

        // Create a PCB
        let pcb: Pcb<SimplePrefix> =
            create_initial_pcb(isd_as1, 1000, InterfaceId(1), 1500);

        // Add peering entries
        let peering_links = vec![
            (isd_as2, InterfaceId(5), InterfaceId(6), 1400),
        ];

        let pcb_with_peers = add_peering_entries(pcb, peering_links, 1000);

        // Verify peer entry was added
        assert_eq!(pcb_with_peers.as_entries.len(), 1);
        let last_entry = pcb_with_peers.as_entries.last().unwrap();
        assert_eq!(last_entry.peer_entries.len(), 1);
        assert_eq!(last_entry.peer_entries[0].peer_isd_as, isd_as2);
        assert_eq!(last_entry.peer_entries[0].peer_interface.0, 6);
        assert_eq!(last_entry.peer_entries[0].peer_mtu, 1400);
    }

    #[test]
    fn test_add_multiple_peering_entries() {
        let isd_as1 = IsdAs::new(1, 110u64);
        let isd_as2 = IsdAs::new(1, 120u64);
        let isd_as3 = IsdAs::new(1, 130u64);

        let pcb: Pcb<SimplePrefix> =
            create_initial_pcb(isd_as1, 1000, InterfaceId(1), 1500);

        // Add multiple peering entries
        let peering_links = vec![
            (isd_as2, InterfaceId(5), InterfaceId(6), 1400),
            (isd_as3, InterfaceId(7), InterfaceId(8), 1300),
        ];

        let pcb_with_peers = add_peering_entries(pcb, peering_links, 1000);

        // Verify both peer entries were added
        let last_entry = pcb_with_peers.as_entries.last().unwrap();
        assert_eq!(last_entry.peer_entries.len(), 2);
        assert_eq!(last_entry.peer_entries[0].peer_isd_as, isd_as2);
        assert_eq!(last_entry.peer_entries[1].peer_isd_as, isd_as3);
    }

    #[test]
    fn test_select_for_propagation() {
        let policy = SimpleSelectionPolicy;

        // Create PCBs with different lengths
        let pcb1: Pcb<SimplePrefix> =
            create_initial_pcb(IsdAs::new(1, 110u64), 1000, InterfaceId(1), 1500);

        let pcb2 = extend_pcb(
            create_initial_pcb(IsdAs::new(1, 120u64), 1000, InterfaceId(1), 1500),
            IsdAs::new(1, 130u64),
            InterfaceId(2),
            InterfaceId(3),
            1500,
            1000,
        );

        let pcbs = vec![pcb1.clone(), pcb2.clone()];

        // Select 1 PCB - should select the shortest
        let selected = select_for_propagation(&pcbs, &policy, 1);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].path_length(), 1);
    }

    #[test]
    fn test_select_for_propagation_empty() {
        let policy = SimpleSelectionPolicy;
        let pcbs: Vec<Pcb<SimplePrefix>> = vec![];

        let selected = select_for_propagation(&pcbs, &policy, 5);
        assert_eq!(selected.len(), 0);
    }
}
