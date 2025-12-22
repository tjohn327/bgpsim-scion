use bgpsim::prelude::*;

// Define the type of the network.
type Prefix = SimplePrefix;           // Use non-overlapping prefixes.
type Queue = BasicEventQueue<Prefix>; // Use a basic FIFO event queue
type Ospf = GlobalOspf;               // Use global OSPF without message passing
type Net = Network<Prefix, Queue, Ospf>;

fn main() -> Result<(), NetworkError> {

    let mut t = Net::default();

    let prefix = Prefix::from(0);

    let e0 = t.add_router("E0", 1);
    let b0 = t.add_router("B0", 65500);
    let r0 = t.add_router("R0", 65500);
    let r1 = t.add_router("R1", 65500);
    let b1 = t.add_router("B1", 65500);
    let e1 = t.add_router("E1", 2);

    t.add_link(e0, b0);
    t.add_link(b0, r0);
    t.add_link(r0, r1);
    t.add_link(r1, b1);
    t.add_link(b1, e1);

    t.set_link_weight(b0, r0, 1.0)?;
    t.set_link_weight(r0, b0, 1.0)?;
    t.set_link_weight(r0, r1, 1.0)?;
    t.set_link_weight(r1, r0, 1.0)?;
    t.set_link_weight(r1, b1, 1.0)?;
    t.set_link_weight(b1, r1, 1.0)?;
    t.set_bgp_session(e0, b0, Some(false))?;
    t.set_bgp_session(r0, b0, Some(true))?;
    t.set_bgp_session(r0, r1, Some(false))?;
    t.set_bgp_session(r1, b1, Some(true))?;
    t.set_bgp_session(e1, b1, Some(false))?;

    // advertise the same prefix on both routers
    t.advertise_route(e0, prefix, &[1, 2, 3], None, None)?;
    t.advertise_route(e1, prefix, &[2, 3], None, None)?;

    // get the forwarding state
    let mut fw_state = t.get_forwarding_state();

    // check that all routes are correct
    assert_eq!(fw_state.get_paths(b0, prefix)?, vec![vec![b0, r0, r1, b1, e1]]);
    assert_eq!(fw_state.get_paths(r0, prefix)?, vec![vec![r0, r1, b1, e1]]);
    assert_eq!(fw_state.get_paths(r1, prefix)?, vec![vec![r1, b1, e1]]);
    assert_eq!(fw_state.get_paths(b1, prefix)?, vec![vec![b1, e1]]);

    println!("Example 1: All assertions passed!");
    Ok(())
}
