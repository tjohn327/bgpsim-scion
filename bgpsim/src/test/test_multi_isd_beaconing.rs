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

//! Test multi-ISD beaconing and inter-ISD path discovery

#[cfg(feature = "scion")]
#[generic_tests::define]
mod t {
    use crate::{
        event::BasicEventQueue,
        network::Network,
        scion::{IsdAs, ScionLinkType},
        types::{Prefix, SimplePrefix, ASN},
    };

    /// Test that multi-ISD beaconing correctly propagates PCBs across ISDs
    ///
    /// Topology:
    /// ```
    /// ISD 1:              ISD 2:
    ///   core1 =========== core2  (core link)
    ///     |                 |
    ///   leaf1             leaf2
    /// ```
    ///
    /// Expected behavior:
    /// 1. Core beaconing: core1 ↔ core2 exchange PCBs
    /// 2. Intra-ISD beaconing: core ASes propagate PCBs (including foreign) to leaves
    /// 3. Registration: non-core ASes register up-segments, cores create down-segments
    /// 4. Path lookup: leaf1 → leaf2 finds inter-ISD paths
    #[test]
    fn test_basic_multi_isd_beaconing<P: Prefix>() {
        // Create network
        let mut net: Network<P, BasicEventQueue<P>, crate::ospf::GlobalOspf> =
            Network::default();

        // ISD 1
        let core1 = net.add_router("core1", ASN(110));
        let leaf1 = net.add_router("leaf1", ASN(111));

        // ISD 2
        let core2 = net.add_router("core2", ASN(210));
        let leaf2 = net.add_router("leaf2", ASN(211));

        // Topology links (using add_link for actual connectivity)
        net.add_link(core1, core2).unwrap();
        net.add_link(core1, leaf1).unwrap();
        net.add_link(core2, leaf2).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1u16, 110u64), true).unwrap(); // Core
        net.enable_scion(leaf1, IsdAs::new(1u16, 111u64), false)
            .unwrap(); // Non-core

        net.enable_scion(core2, IsdAs::new(2u16, 210u64), true).unwrap(); // Core
        net.enable_scion(leaf2, IsdAs::new(2u16, 211u64), false)
            .unwrap(); // Non-core

        // Configure SCION links
        net.configure_scion_link(core1, core2, ScionLinkType::Core)
            .unwrap(); // Inter-ISD
        net.configure_scion_link(core1, leaf1, ScionLinkType::ParentChild)
            .unwrap();
        net.configure_scion_link(core2, leaf2, ScionLinkType::ParentChild)
            .unwrap();

        // Core beaconing (creates initial PCBs between core ASes)
        let core_pcbs = net.scion_core_beaconing(1000).unwrap();
        println!("Core beaconing created {} PCBs", core_pcbs);
        assert!(core_pcbs > 0, "Core beaconing should create PCBs");

        // Intra-ISD beaconing (propagates PCBs top-down)
        // With fix: core ASes propagate foreign ISD PCBs to leaves
        let intra_pcbs = net.scion_intra_isd_beaconing(1000, 50).unwrap();
        println!("Intra-ISD beaconing propagated {} PCBs", intra_pcbs);
        assert!(
            intra_pcbs > 0,
            "Intra-ISD beaconing should propagate PCBs to leaves"
        );

        // Registration (convert PCBs to path segments)
        let (up_count, down_count, core_count) = net.scion_registration_round(50).unwrap();
        println!(
            "Registered: {} up, {} down, {} core segments",
            up_count, down_count, core_count
        );

        // Verify segments were registered
        assert!(up_count > 0, "Should have up-segments (leaf → core)");
        assert!(down_count > 0, "Should have down-segments (core → leaf)");
        // Core segments might be 0 in this minimal topology - that's ok

        // Verify inter-ISD path lookup works
        let paths = net.scion_lookup_paths(leaf1, leaf2).unwrap();
        println!("Found {} paths from leaf1 to leaf2", paths.len());
        assert!(
            !paths.is_empty(),
            "Should find at least one inter-ISD path"
        );

        // Verify reverse direction works
        let paths_reverse = net.scion_lookup_paths(leaf2, leaf1).unwrap();
        assert!(
            !paths_reverse.is_empty(),
            "Should find paths in reverse direction too"
        );
    }

    /// Test 3-ISD topology to verify transitive inter-ISD connectivity
    ///
    /// Topology:
    /// ```
    /// ISD 1:         ISD 2:         ISD 3:
    ///  core1 ======= core2 ======= core3
    ///    |             |             |
    ///  leaf1         leaf2         leaf3
    /// ```
    #[test]
    fn test_three_isd_beaconing<P: Prefix>() {
        let mut net: Network<P, BasicEventQueue<P>, crate::ospf::GlobalOspf> =
            Network::default();

        // Create routers
        let core1 = net.add_router("core1", ASN(110));
        let leaf1 = net.add_router("leaf1", ASN(111));
        let core2 = net.add_router("core2", ASN(210));
        let leaf2 = net.add_router("leaf2", ASN(211));
        let core3 = net.add_router("core3", ASN(310));
        let leaf3 = net.add_router("leaf3", ASN(311));

        // Topology
        net.add_link(core1, core2).unwrap();
        net.add_link(core2, core3).unwrap();
        net.add_link(core1, leaf1).unwrap();
        net.add_link(core2, leaf2).unwrap();
        net.add_link(core3, leaf3).unwrap();

        // Enable SCION
        net.enable_scion(core1, IsdAs::new(1u16, 110u64), true).unwrap();
        net.enable_scion(leaf1, IsdAs::new(1u16, 111u64), false).unwrap();
        net.enable_scion(core2, IsdAs::new(2u16, 210u64), true).unwrap();
        net.enable_scion(leaf2, IsdAs::new(2u16, 211u64), false).unwrap();
        net.enable_scion(core3, IsdAs::new(3u16, 310u64), true).unwrap();
        net.enable_scion(leaf3, IsdAs::new(3u16, 311u64), false).unwrap();

        // Configure SCION links
        net.configure_scion_link(core1, core2, ScionLinkType::Core)
            .unwrap();
        net.configure_scion_link(core2, core3, ScionLinkType::Core)
            .unwrap();
        net.configure_scion_link(core1, leaf1, ScionLinkType::ParentChild)
            .unwrap();
        net.configure_scion_link(core2, leaf2, ScionLinkType::ParentChild)
            .unwrap();
        net.configure_scion_link(core3, leaf3, ScionLinkType::ParentChild)
            .unwrap();

        // Beaconing
        net.scion_core_beaconing(1000).unwrap();

        // Need 2 rounds for PCBs to propagate through the chain
        net.scion_intra_isd_beaconing(1000, 50).unwrap();
        net.scion_intra_isd_beaconing(2000, 50).unwrap();

        let (up, down, _core) = net.scion_registration_round(50).unwrap();
        assert!(up > 0, "Should register up-segments in 3-ISD topology");
        assert!(down > 0, "Should register down-segments in 3-ISD topology");

        // Verify paths exist between all pairs
        let pairs = vec![
            (leaf1, leaf2),
            (leaf1, leaf3),
            (leaf2, leaf3),
            (leaf2, leaf1),
            (leaf3, leaf1),
            (leaf3, leaf2),
        ];

        for (src, dst) in pairs {
            let paths = net.scion_lookup_paths(src, dst).unwrap();
            assert!(
                !paths.is_empty(),
                "Should find path from {:?} to {:?}",
                src,
                dst
            );
        }
    }

    /// Test that non-core ASes still filter same-ISD (regression test)
    ///
    /// Ensures the fix doesn't break single-ISD behavior
    #[test]
    fn test_single_isd_still_works<P: Prefix>() {
        let mut net: Network<P, BasicEventQueue<P>, crate::ospf::GlobalOspf> =
            Network::default();

        // Single ISD topology
        let core = net.add_router("core", ASN(110));
        let transit = net.add_router("transit", ASN(120));
        let leaf = net.add_router("leaf", ASN(130));

        net.add_link(core, transit).unwrap();
        net.add_link(transit, leaf).unwrap();

        // All same ISD
        net.enable_scion(core, IsdAs::new(1u16, 110u64), true).unwrap();
        net.enable_scion(transit, IsdAs::new(1u16, 120u64), false)
            .unwrap();
        net.enable_scion(leaf, IsdAs::new(1u16, 130u64), false).unwrap();

        net.configure_scion_link(core, transit, ScionLinkType::ParentChild)
            .unwrap();
        net.configure_scion_link(transit, leaf, ScionLinkType::ParentChild)
            .unwrap();

        // Beaconing
        net.scion_core_beaconing(1000).unwrap();
        net.scion_intra_isd_beaconing(1000, 50).unwrap();
        net.scion_intra_isd_beaconing(2000, 50).unwrap(); // 2nd round for transit propagation

        let (up, down, _) = net.scion_registration_round(50).unwrap();
        assert!(up > 0, "Single-ISD should still work");
        assert!(down > 0, "Single-ISD should still work");

        let paths = net.scion_lookup_intra_isd_paths(leaf, core).unwrap();
        assert!(!paths.is_empty(), "Should find intra-ISD paths");
    }

    #[instantiate_tests(<crate::types::SimplePrefix>)]
    mod simple_prefix {}
}
