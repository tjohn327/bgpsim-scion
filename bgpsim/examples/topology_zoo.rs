use bgpsim::prelude::*;
use bgpsim::topology_zoo::TopologyZoo;

type Prefix = SimplePrefix;           // Use non-overlapping prefixes.
type Queue = BasicEventQueue<Prefix>; // Use a basic FIFO event queue
type Ospf = GlobalOspf;               // Use global OSPF without message passing
type Net = Network<Prefix, Queue, Ospf>;

fn main() -> Result<(), NetworkError> {

    // create the Abilene network from TopologyZoo
    // Abilene has only internal routers, so external ASNs don't matter
    let net: Net = TopologyZoo::Abilene.build(Queue::new(), 65500, 1);

    println!("Example 3: TopologyZoo Abilene network created successfully!");
    println!("Network has {} routers", net.routers().count());
    println!("Network has {} links", net.get_topology().edge_count());

    Ok(())
}
