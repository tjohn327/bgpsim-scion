// Multi-ISD, Multi-Router AS Test
//
// This example demonstrates the multi-router AS functionality with a realistic
// SCION topology spanning 3 ISDs with various AS types (core, tier2, leaf, CDN).
//
// Key features:
// - Multiple border routers per AS (demonstrates new multi-router AS support)
// - Inter-ISD and intra-ISD connectivity
// - Various AS roles: core, transit, access, enterprise, CDN
// - Comprehensive path lookup validation

use bgpsim::prelude::*;
use bgpsim::scion::{IsdAs, ScionLinkType};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Multi-ISD, Multi-Router AS SCION Test ===\n");

    // Create network
    let mut net = Network::<SimplePrefix, BasicEventQueue<_>, GlobalOspf>::default();

    // Track routers by AS for easier management
    let mut as_routers: std::collections::HashMap<IsdAs, Vec<RouterId>> =
        std::collections::HashMap::new();

    println!("Phase 1: Creating topology...");

    // ========== ISD 1 ==========
    println!("  Creating ISD 1 ASes...");

    // ISD 1, AS 1: ISP-A-Core (5 routers: 3 border, 2 internal)
    let isd1_as1 = IsdAs::new(1u16, 1u32);
    let mut isd1_as1_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD1-AS1-br{}", i), ASN(1));
        isd1_as1_routers.push(router);
    }
    // Internal routers (for topology only, no SCION)
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS1-ir{}", i), ASN(1));
        isd1_as1_routers.push(router);
    }
    as_routers.insert(isd1_as1, isd1_as1_routers.clone());

    // ISD 1, AS 2: ISP-B-Core
    let isd1_as2 = IsdAs::new(1u16, 2u32);
    let mut isd1_as2_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD1-AS2-br{}", i), ASN(2));
        isd1_as2_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS2-ir{}", i), ASN(2));
        isd1_as2_routers.push(router);
    }
    as_routers.insert(isd1_as2, isd1_as2_routers.clone());

    // ISD 1, AS 3: IX-Core
    let isd1_as3 = IsdAs::new(1u16, 3u32);
    let mut isd1_as3_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD1-AS3-br{}", i), ASN(3));
        isd1_as3_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS3-ir{}", i), ASN(3));
        isd1_as3_routers.push(router);
    }
    as_routers.insert(isd1_as3, isd1_as3_routers.clone());

    // ISD 1, AS 10: Regional-ISP-R1 (Tier 2)
    let isd1_as10 = IsdAs::new(1u16, 10u32);
    let mut isd1_as10_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD1-AS10-br{}", i), ASN(10));
        isd1_as10_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS10-ir{}", i), ASN(10));
        isd1_as10_routers.push(router);
    }
    as_routers.insert(isd1_as10, isd1_as10_routers.clone());

    // ISD 1, AS 11: Regional-ISP-R2 (Tier 2)
    let isd1_as11 = IsdAs::new(1u16, 11u32);
    let mut isd1_as11_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD1-AS11-br{}", i), ASN(11));
        isd1_as11_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS11-ir{}", i), ASN(11));
        isd1_as11_routers.push(router);
    }
    as_routers.insert(isd1_as11, isd1_as11_routers.clone());

    // ISD 1, AS 12: Enterprise-DC-E1 (Tier 2)
    let isd1_as12 = IsdAs::new(1u16, 12u32);
    let mut isd1_as12_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD1-AS12-br{}", i), ASN(12));
        isd1_as12_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS12-ir{}", i), ASN(12));
        isd1_as12_routers.push(router);
    }
    as_routers.insert(isd1_as12, isd1_as12_routers.clone());

    // ISD 1, AS 20: Access-A1-FTTH-DSL (Leaf)
    let isd1_as20 = IsdAs::new(1u16, 20u32);
    let mut isd1_as20_routers = Vec::new();
    isd1_as20_routers.push(net.add_router("ISD1-AS20-br1".to_string(), ASN(20)));
    for i in 1..=3 {
        let router = net.add_router(format!("ISD1-AS20-ir{}", i), ASN(20));
        isd1_as20_routers.push(router);
    }
    as_routers.insert(isd1_as20, isd1_as20_routers.clone());

    // ISD 1, AS 21: Access-A2-Mobile (Leaf)
    let isd1_as21 = IsdAs::new(1u16, 21u32);
    let mut isd1_as21_routers = Vec::new();
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS21-br{}", i), ASN(21));
        isd1_as21_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS21-ir{}", i), ASN(21));
        isd1_as21_routers.push(router);
    }
    as_routers.insert(isd1_as21, isd1_as21_routers.clone());

    // ISD 1, AS 30: Small-Enterprise-S1 (Leaf)
    let isd1_as30 = IsdAs::new(1u16, 30u32);
    let mut isd1_as30_routers = Vec::new();
    isd1_as30_routers.push(net.add_router("ISD1-AS30-br1".to_string(), ASN(30)));
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS30-ir{}", i), ASN(30));
        isd1_as30_routers.push(router);
    }
    as_routers.insert(isd1_as30, isd1_as30_routers.clone());

    // ISD 1, AS 31: Campus-C1-University (Leaf)
    let isd1_as31 = IsdAs::new(1u16, 31u32);
    let mut isd1_as31_routers = Vec::new();
    isd1_as31_routers.push(net.add_router("ISD1-AS31-br1".to_string(), ASN(31)));
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS31-ir{}", i), ASN(31));
        isd1_as31_routers.push(router);
    }
    as_routers.insert(isd1_as31, isd1_as31_routers.clone());

    // ISD 1, AS 100: CDN-POP-ISD1
    let isd1_as100 = IsdAs::new(1u16, 100u32);
    let mut isd1_as100_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD1-AS100-br{}", i), ASN(100));
        isd1_as100_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD1-AS100-ir{}", i), ASN(100));
        isd1_as100_routers.push(router);
    }
    as_routers.insert(isd1_as100, isd1_as100_routers.clone());

    // ========== ISD 2 ==========
    println!("  Creating ISD 2 ASes...");

    // ISD 2, AS 1: Transit-ISP-T1 (Core)
    let isd2_as1 = IsdAs::new(2u16, 1u32);
    let mut isd2_as1_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD2-AS1-br{}", i), ASN(201));
        isd2_as1_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD2-AS1-ir{}", i), ASN(201));
        isd2_as1_routers.push(router);
    }
    as_routers.insert(isd2_as1, isd2_as1_routers.clone());

    // ISD 2, AS 2: Transit-ISP-T2 (Core)
    let isd2_as2 = IsdAs::new(2u16, 2u32);
    let mut isd2_as2_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD2-AS2-br{}", i), ASN(202));
        isd2_as2_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD2-AS2-ir{}", i), ASN(202));
        isd2_as2_routers.push(router);
    }
    as_routers.insert(isd2_as2, isd2_as2_routers.clone());

    // ISD 2, AS 10: Regional-ISP-R3 (Tier 2)
    let isd2_as10 = IsdAs::new(2u16, 10u32);
    let mut isd2_as10_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD2-AS10-br{}", i), ASN(210));
        isd2_as10_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD2-AS10-ir{}", i), ASN(210));
        isd2_as10_routers.push(router);
    }
    as_routers.insert(isd2_as10, isd2_as10_routers.clone());

    // ISD 2, AS 11: Cloud-Hosting-H1 (Tier 2)
    let isd2_as11 = IsdAs::new(2u16, 11u32);
    let mut isd2_as11_routers = Vec::new();
    for i in 1..=2 {
        let router = net.add_router(format!("ISD2-AS11-br{}", i), ASN(211));
        isd2_as11_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD2-AS11-ir{}", i), ASN(211));
        isd2_as11_routers.push(router);
    }
    as_routers.insert(isd2_as11, isd2_as11_routers.clone());

    // ISD 2, AS 20: Access-A3 (Leaf)
    let isd2_as20 = IsdAs::new(2u16, 20u32);
    let mut isd2_as20_routers = Vec::new();
    isd2_as20_routers.push(net.add_router("ISD2-AS20-br1".to_string(), ASN(220)));
    for i in 1..=3 {
        let router = net.add_router(format!("ISD2-AS20-ir{}", i), ASN(220));
        isd2_as20_routers.push(router);
    }
    as_routers.insert(isd2_as20, isd2_as20_routers.clone());

    // ISD 2, AS 21: Access-A4-Rural-Wireless (Leaf)
    let isd2_as21 = IsdAs::new(2u16, 21u32);
    let mut isd2_as21_routers = Vec::new();
    isd2_as21_routers.push(net.add_router("ISD2-AS21-br1".to_string(), ASN(221)));
    for i in 1..=3 {
        let router = net.add_router(format!("ISD2-AS21-ir{}", i), ASN(221));
        isd2_as21_routers.push(router);
    }
    as_routers.insert(isd2_as21, isd2_as21_routers.clone());

    // ISD 2, AS 30: Enterprise-S2 (Leaf)
    let isd2_as30 = IsdAs::new(2u16, 30u32);
    let mut isd2_as30_routers = Vec::new();
    isd2_as30_routers.push(net.add_router("ISD2-AS30-br1".to_string(), ASN(230)));
    for i in 1..=2 {
        let router = net.add_router(format!("ISD2-AS30-ir{}", i), ASN(230));
        isd2_as30_routers.push(router);
    }
    as_routers.insert(isd2_as30, isd2_as30_routers.clone());

    // ISD 2, AS 100: CDN-POP-ISD2
    let isd2_as100 = IsdAs::new(2u16, 100u32);
    let mut isd2_as100_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD2-AS100-br{}", i), ASN(300));
        isd2_as100_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD2-AS100-ir{}", i), ASN(300));
        isd2_as100_routers.push(router);
    }
    as_routers.insert(isd2_as100, isd2_as100_routers.clone());

    // ========== ISD 3 ==========
    println!("  Creating ISD 3 ASes...");

    // ISD 3, AS 1: Regional-ISP-R4-Core (Core)
    let isd3_as1 = IsdAs::new(3u16, 1u32);
    let mut isd3_as1_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD3-AS1-br{}", i), ASN(301));
        isd3_as1_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD3-AS1-ir{}", i), ASN(301));
        isd3_as1_routers.push(router);
    }
    as_routers.insert(isd3_as1, isd3_as1_routers.clone());

    // ISD 3, AS 10: Local-ISP-L1 (Tier 2)
    let isd3_as10 = IsdAs::new(3u16, 10u32);
    let mut isd3_as10_routers = Vec::new();
    for i in 1..=3 {
        let router = net.add_router(format!("ISD3-AS10-br{}", i), ASN(310));
        isd3_as10_routers.push(router);
    }
    for i in 1..=2 {
        let router = net.add_router(format!("ISD3-AS10-ir{}", i), ASN(310));
        isd3_as10_routers.push(router);
    }
    as_routers.insert(isd3_as10, isd3_as10_routers.clone());

    // ISD 3, AS 20: Access-A5 (Leaf)
    let isd3_as20 = IsdAs::new(3u16, 20u32);
    let mut isd3_as20_routers = Vec::new();
    isd3_as20_routers.push(net.add_router("ISD3-AS20-br1".to_string(), ASN(320)));
    for i in 1..=3 {
        let router = net.add_router(format!("ISD3-AS20-ir{}", i), ASN(320));
        isd3_as20_routers.push(router);
    }
    as_routers.insert(isd3_as20, isd3_as20_routers.clone());

    // ISD 3, AS 30: Enterprise-S3 (Leaf)
    let isd3_as30 = IsdAs::new(3u16, 30u32);
    let mut isd3_as30_routers = Vec::new();
    isd3_as30_routers.push(net.add_router("ISD3-AS30-br1".to_string(), ASN(330)));
    for i in 1..=2 {
        let router = net.add_router(format!("ISD3-AS30-ir{}", i), ASN(330));
        isd3_as30_routers.push(router);
    }
    as_routers.insert(isd3_as30, isd3_as30_routers.clone());

    // ISD 3, AS 100: CDN-POP-ISD3-Small
    let isd3_as100 = IsdAs::new(3u16, 100u32);
    let mut isd3_as100_routers = Vec::new();
    for i in 1..=2 {
        let router = net.add_router(format!("ISD3-AS100-br{}", i), ASN(400));
        isd3_as100_routers.push(router);
    }
    isd3_as100_routers.push(net.add_router("ISD3-AS100-ir1".to_string(), ASN(400)));
    as_routers.insert(isd3_as100, isd3_as100_routers.clone());

    println!("  Total routers created: {}", net.num_routers());
    println!("  Total ASes: {}\n", as_routers.len());

    // Phase 2: Enable SCION on all border routers (only first 3 routers per AS are border routers)
    println!("Phase 2: Enabling SCION...");

    // Helper to enable SCION on border routers of an AS
    let enable_scion_for_as = |net: &mut Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf>,
                               isd_as: IsdAs,
                               routers: &[RouterId],
                               is_core: bool,
                               num_border: usize| {
        for (idx, &router) in routers.iter().enumerate() {
            if idx < num_border {
                // Border router - enable SCION
                net.enable_scion(router, isd_as, is_core).unwrap();
            }
        }
    };

    // ISD 1 - Core ASes
    enable_scion_for_as(&mut net, isd1_as1, &isd1_as1_routers, true, 3);
    enable_scion_for_as(&mut net, isd1_as2, &isd1_as2_routers, true, 3);
    enable_scion_for_as(&mut net, isd1_as3, &isd1_as3_routers, true, 3);

    // ISD 1 - Tier 2
    enable_scion_for_as(&mut net, isd1_as10, &isd1_as10_routers, false, 3);
    enable_scion_for_as(&mut net, isd1_as11, &isd1_as11_routers, false, 3);
    enable_scion_for_as(&mut net, isd1_as12, &isd1_as12_routers, false, 3);

    // ISD 1 - Leaf
    enable_scion_for_as(&mut net, isd1_as20, &isd1_as20_routers, false, 1);
    enable_scion_for_as(&mut net, isd1_as21, &isd1_as21_routers, false, 2);
    enable_scion_for_as(&mut net, isd1_as30, &isd1_as30_routers, false, 1);
    enable_scion_for_as(&mut net, isd1_as31, &isd1_as31_routers, false, 1);

    // ISD 1 - CDN
    enable_scion_for_as(&mut net, isd1_as100, &isd1_as100_routers, false, 3);

    // ISD 2 - Core ASes
    enable_scion_for_as(&mut net, isd2_as1, &isd2_as1_routers, true, 3);
    enable_scion_for_as(&mut net, isd2_as2, &isd2_as2_routers, true, 3);

    // ISD 2 - Tier 2
    enable_scion_for_as(&mut net, isd2_as10, &isd2_as10_routers, false, 3);
    enable_scion_for_as(&mut net, isd2_as11, &isd2_as11_routers, false, 2);

    // ISD 2 - Leaf
    enable_scion_for_as(&mut net, isd2_as20, &isd2_as20_routers, false, 1);
    enable_scion_for_as(&mut net, isd2_as21, &isd2_as21_routers, false, 1);
    enable_scion_for_as(&mut net, isd2_as30, &isd2_as30_routers, false, 1);

    // ISD 2 - CDN
    enable_scion_for_as(&mut net, isd2_as100, &isd2_as100_routers, false, 3);

    // ISD 3 - Core AS
    enable_scion_for_as(&mut net, isd3_as1, &isd3_as1_routers, true, 3);

    // ISD 3 - Tier 2
    enable_scion_for_as(&mut net, isd3_as10, &isd3_as10_routers, false, 3);

    // ISD 3 - Leaf
    enable_scion_for_as(&mut net, isd3_as20, &isd3_as20_routers, false, 1);
    enable_scion_for_as(&mut net, isd3_as30, &isd3_as30_routers, false, 1);

    // ISD 3 - CDN
    enable_scion_for_as(&mut net, isd3_as100, &isd3_as100_routers, false, 2);

    println!("  SCION enabled on border routers");
    println!("  Total SCION ASes: {}\n", net.scion_as_count());

    // Phase 3: Configure SCION links
    println!("Phase 3: Configuring SCION links...");

    let mut link_count = 0;

    // Helper to configure link (both network topology and SCION)
    let configure_link = |net: &mut Network<SimplePrefix, BasicEventQueue<_>, GlobalOspf>,
                          routers_a: &[RouterId],
                          idx_a: usize,
                          routers_b: &[RouterId],
                          idx_b: usize,
                          link_type: ScionLinkType| {
        // Add network topology link
        net.add_link(routers_a[idx_a], routers_b[idx_b]).unwrap();
        // Add SCION link
        net.configure_scion_link(routers_a[idx_a], routers_b[idx_b], link_type)
            .unwrap();
    };

    // ISD 1 core mesh
    configure_link(
        &mut net,
        &isd1_as1_routers,
        0,
        &isd1_as2_routers,
        0,
        ScionLinkType::Core,
    );
    configure_link(
        &mut net,
        &isd1_as2_routers,
        0,
        &isd1_as3_routers,
        0,
        ScionLinkType::Core,
    );
    configure_link(
        &mut net,
        &isd1_as1_routers,
        0,
        &isd1_as3_routers,
        0,
        ScionLinkType::Core,
    );
    link_count += 3;

    // ISD 1 Tier2 to core
    configure_link(
        &mut net,
        &isd1_as10_routers,
        0,
        &isd1_as1_routers,
        1,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as10_routers,
        1,
        &isd1_as3_routers,
        1,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as11_routers,
        0,
        &isd1_as2_routers,
        1,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as12_routers,
        0,
        &isd1_as1_routers,
        1,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as12_routers,
        1,
        &isd1_as2_routers,
        1,
        ScionLinkType::ParentChild,
    );
    link_count += 5;

    // ISD 1 Leaf to tier2
    configure_link(
        &mut net,
        &isd1_as20_routers,
        0,
        &isd1_as10_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as21_routers,
        0,
        &isd1_as10_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as21_routers,
        1,
        &isd1_as11_routers,
        1,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as30_routers,
        0,
        &isd1_as11_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as31_routers,
        0,
        &isd1_as12_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as100_routers,
        0,
        &isd1_as20_routers,
        0,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd1_as100_routers,
        1,
        &isd1_as21_routers,
        0,
        ScionLinkType::ParentChild,
    );
    link_count += 7;

    // ISD 1 CDN to cores
    configure_link(
        &mut net,
        &isd1_as100_routers,
        0,
        &isd1_as1_routers,
        1,
        ScionLinkType::Peering,
    );
    configure_link(
        &mut net,
        &isd1_as100_routers,
        1,
        &isd1_as3_routers,
        2,
        ScionLinkType::Peering,
    );
    link_count += 2;

    // ISD 2 core mesh
    configure_link(
        &mut net,
        &isd2_as1_routers,
        2,
        &isd2_as2_routers,
        0,
        ScionLinkType::Core,
    );
    link_count += 1;

    // ISD 2 Tier2 to core
    configure_link(
        &mut net,
        &isd2_as10_routers,
        0,
        &isd2_as1_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd2_as10_routers,
        1,
        &isd2_as2_routers,
        1,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd2_as11_routers,
        0,
        &isd2_as1_routers,
        2,
        ScionLinkType::ParentChild,
    );
    link_count += 3;

    // ISD 2 Leaf to tier2
    configure_link(
        &mut net,
        &isd2_as20_routers,
        0,
        &isd2_as10_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd2_as21_routers,
        0,
        &isd2_as10_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd2_as30_routers,
        0,
        &isd2_as11_routers,
        1,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd2_as100_routers,
        0,
        &isd2_as20_routers,
        0,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd2_as100_routers,
        1,
        &isd2_as21_routers,
        0,
        ScionLinkType::ParentChild,
    );
    link_count += 5;

    // ISD 2 CDN to cores
    configure_link(
        &mut net,
        &isd2_as100_routers,
        0,
        &isd2_as1_routers,
        2,
        ScionLinkType::Peering,
    );
    configure_link(
        &mut net,
        &isd2_as100_routers,
        1,
        &isd2_as2_routers,
        1,
        ScionLinkType::Peering,
    );
    link_count += 2;

    // ISD 3 links
    configure_link(
        &mut net,
        &isd3_as10_routers,
        0,
        &isd3_as1_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd3_as20_routers,
        0,
        &isd3_as10_routers,
        1,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd3_as30_routers,
        0,
        &isd3_as10_routers,
        2,
        ScionLinkType::ParentChild,
    );
    configure_link(
        &mut net,
        &isd3_as100_routers,
        0,
        &isd3_as1_routers,
        2,
        ScionLinkType::Peering,
    );
    configure_link(
        &mut net,
        &isd3_as100_routers,
        1,
        &isd3_as20_routers,
        0,
        ScionLinkType::ParentChild,
    );
    link_count += 5;

    // Inter-ISD core links
    configure_link(
        &mut net,
        &isd1_as1_routers,
        2,
        &isd2_as1_routers,
        0,
        ScionLinkType::Core,
    );
    configure_link(
        &mut net,
        &isd1_as2_routers,
        1,
        &isd2_as2_routers,
        0,
        ScionLinkType::Core,
    );
    configure_link(
        &mut net,
        &isd1_as3_routers,
        0,
        &isd3_as1_routers,
        0,
        ScionLinkType::Core,
    );
    configure_link(
        &mut net,
        &isd2_as1_routers,
        1,
        &isd3_as1_routers,
        1,
        ScionLinkType::Core,
    );
    link_count += 4;

    // Inter-ISD CDN links
    configure_link(
        &mut net,
        &isd1_as100_routers,
        2,
        &isd2_as100_routers,
        2,
        ScionLinkType::Peering,
    );
    configure_link(
        &mut net,
        &isd2_as100_routers,
        2,
        &isd3_as100_routers,
        1,
        ScionLinkType::Peering,
    );
    link_count += 2;

    println!("  Total SCION links configured: {}\n", link_count);

    // Phase 4: Run beaconing (event-driven approach)
    println!("Phase 4: Running beaconing...");

    // Start beaconing and converge (event-driven)
    let core_count = net.scion_start_beaconing(1000)?;
    println!("  Core ASes started beaconing: {}", core_count);

    let events_processed = net.scion_converge()?;
    println!("  Events processed: {}", events_processed);

    // Count total PCBs stored at AS level
    let mut total_pcbs = 0;
    let as_ids: Vec<IsdAs> = vec![
        isd1_as1, isd1_as2, isd1_as3, isd1_as10, isd1_as11, isd1_as12, isd1_as20, isd1_as21,
        isd1_as30, isd1_as31, isd1_as100, isd2_as1, isd2_as2, isd2_as10, isd2_as11, isd2_as20,
        isd2_as21, isd2_as30, isd2_as100, isd3_as1, isd3_as10, isd3_as20, isd3_as30, isd3_as100,
    ];
    for isd_as in &as_ids {
        if let Some(scion_as) = net.get_scion_as(isd_as) {
            total_pcbs += scion_as.control_service.beacon_store.total_count();
        }
    }
    println!("  Total PCBs stored (AS-level): {}", total_pcbs);

    // Registration (using batch registration for now)
    let (up_segs, down_segs, core_segs) = net.scion_registration_round(50)?;
    println!(
        "  Registration: {} up, {} down, {} core segments registered\n",
        up_segs, down_segs, core_segs
    );

    // DEBUG: Examine segment storage for key ASes
    println!("=== DEBUG: Segment Storage Analysis ===\n");

    let debug_ases = vec![
        (isd1_as1, "ISD1-AS1 (Core)"),
        (isd1_as2, "ISD1-AS2 (Core)"),
        (isd1_as10, "ISD1-AS10 (Tier2)"),
        (isd1_as20, "ISD1-AS20 (Leaf)"),
        (isd1_as21, "ISD1-AS21 (Leaf)"),
        (isd1_as30, "ISD1-AS30 (Leaf)"),
        (isd2_as1, "ISD2-AS1 (Core)"),
        (isd2_as2, "ISD2-AS2 (Core)"),
        (isd2_as20, "ISD2-AS20 (Leaf)"),
    ];

    for (isd_as, label) in debug_ases {
        if let Some(scion_as) = net.get_scion_as(&isd_as) {
            let up_segs = scion_as.control_service.path_database.get_all_up_segments();
            let down_segs = scion_as
                .control_service
                .path_database
                .get_all_down_segments();
            let core_segs = scion_as
                .control_service
                .path_database
                .get_all_core_segments();

            println!("{} - {:?}", label, isd_as);
            println!("  Up segments: {} total", up_segs.len());
            if !up_segs.is_empty() {
                for seg in up_segs.iter().take(3) {
                    println!("    {:?} → {:?}", seg.source(), seg.destination());
                }
                if up_segs.len() > 3 {
                    println!("    ... and {} more", up_segs.len() - 3);
                }
            }
            println!("  Down segments: {} total", down_segs.len());
            if !down_segs.is_empty() {
                for seg in down_segs.iter().take(3) {
                    println!("    {:?} → {:?}", seg.source(), seg.destination());
                }
                if down_segs.len() > 3 {
                    println!("    ... and {} more", down_segs.len() - 3);
                }
            }
            println!("  Core segments: {} total", core_segs.len());
            if !core_segs.is_empty() {
                for seg in core_segs.iter().take(3) {
                    println!("    {:?} → {:?}", seg.source(), seg.destination());
                }
                if core_segs.len() > 3 {
                    println!("    ... and {} more", core_segs.len() - 3);
                }
            }
            println!();
        }
    }

    println!("=== End Debug ===\n");

    // Phase 5: Path lookup validation
    println!("Phase 5: Validating path lookups...\n");

    let mut tests_passed = 0;
    let mut tests_total = 0;

    // Test 1: Intra-ISD, same-tier (ISD1 leaf to leaf via tier2)
    // NOTE: Path lookup now operates at AS level (ISD-AS), not router level
    tests_total += 1;
    match net.scion_lookup_paths(isd1_as20, isd1_as21) {
        Ok(paths) => {
            println!("  ✓ Test 1: ISD1 leaf-to-leaf (AS20→AS21)");
            println!("    Found {} paths", paths.len());
            if !paths.is_empty() {
                tests_passed += 1;
            }
        }
        Err(e) => println!("  ✗ Test 1 failed: {:?}", e),
    }

    // Test 2: Intra-ISD, leaf to core (ISD1 AS30 to AS1)
    tests_total += 1;
    match net.scion_lookup_paths(isd1_as30, isd1_as1) {
        Ok(paths) => {
            println!("  ✓ Test 2: ISD1 leaf-to-core (AS30→AS1)");
            println!("    Found {} paths", paths.len());
            if !paths.is_empty() {
                tests_passed += 1;
            }
        }
        Err(e) => println!("  ✗ Test 2 failed: {:?}", e),
    }

    // Test 3: Inter-ISD (ISD1 leaf to ISD2 leaf)
    tests_total += 1;
    println!("\n=== DEBUG: Test 3 Inter-ISD Path Lookup ===");
    println!("Source: ISD1-AS20, Destination: ISD2-AS20");

    // Check what segments are available at source cores
    if let Some(isd1_core1) = net.get_scion_as(&isd1_as1) {
        let core_segs = isd1_core1
            .control_service
            .path_database
            .get_all_core_segments();
        println!("ISD1-AS1 has {} core segments", core_segs.len());
        for (i, seg) in core_segs.iter().take(5).enumerate() {
            println!(
                "  Core seg {}: {:?} → {:?}",
                i + 1,
                seg.source(),
                seg.destination()
            );
        }
    }

    match net.scion_lookup_paths(isd1_as20, isd2_as20) {
        Ok(paths) => {
            println!("  ✓ Test 3: Inter-ISD leaf-to-leaf (ISD1-AS20→ISD2-AS20)");
            println!("    Found {} paths", paths.len());
            if !paths.is_empty() {
                tests_passed += 1;
            }
        }
        Err(e) => println!("  ✗ Test 3 failed: {:?}", e),
    }
    println!("=== End Debug ===\n");

    // Test 4: Inter-ISD (ISD1 enterprise to ISD3 enterprise)
    tests_total += 1;
    match net.scion_lookup_paths(isd1_as30, isd3_as30) {
        Ok(paths) => {
            println!("  ✓ Test 4: Inter-ISD enterprise-to-enterprise (ISD1-AS30→ISD3-AS30)");
            println!("    Found {} paths", paths.len());
            if !paths.is_empty() {
                tests_passed += 1;
            }
        }
        Err(e) => println!("  ✗ Test 4 failed: {:?}", e),
    }

    // Test 5: CDN to CDN across ISDs
    tests_total += 1;
    match net.scion_lookup_paths(isd1_as100, isd2_as100) {
        Ok(paths) => {
            println!("  ✓ Test 5: Inter-ISD CDN-to-CDN (ISD1-AS100→ISD2-AS100)");
            println!("    Found {} paths", paths.len());
            if !paths.is_empty() {
                tests_passed += 1;
            }
        }
        Err(e) => println!("  ✗ Test 5 failed: {:?}", e),
    }

    // Test 6: Verify AS-level path lookup works (paths are between ASes, not routers)
    // This demonstrates that path lookup now operates at the AS level
    tests_total += 1;
    let paths = net.scion_lookup_paths(isd1_as1, isd2_as1)?;
    if !paths.is_empty() {
        println!("  ✓ Test 6: AS-level path lookup (ISD1-AS1 → ISD2-AS1)");
        println!("    AS-level lookup found {} paths", paths.len());
        println!("    (All border routers in ISD1-AS1 can use these paths to reach ISD2-AS1)");
        tests_passed += 1;
    } else {
        println!("  ✗ Test 6 failed: AS-level path lookup found no paths");
    }

    println!("\n=== Test Results ===");
    println!("Tests passed: {}/{}", tests_passed, tests_total);
    println!(
        "Success rate: {:.1}%",
        (tests_passed as f64 / tests_total as f64) * 100.0
    );

    println!("\n=== Scalability Metrics ===");
    println!("Total routers: {}", net.num_routers());
    println!("Total SCION ASes: {}", net.scion_as_count());
    println!(
        "Average routers per AS: {:.1}",
        net.num_routers() as f64 / net.scion_as_count() as f64
    );
    println!("Core ASes: {}", core_count);
    println!("Events processed: {}", events_processed);
    println!("Total PCBs stored: {}", total_pcbs);
    println!(
        "Segments registered: {} up, {} down, {} core",
        up_segs, down_segs, core_segs
    );

    if tests_passed == tests_total {
        println!("\n✅ All tests PASSED!");
        Ok(())
    } else {
        println!("\n❌ Some tests FAILED");
        Err("Some tests failed".into())
    }
}
