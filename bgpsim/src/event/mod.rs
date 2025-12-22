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

//! Module for defining events

use serde::{Deserialize, Serialize};

mod queue;
pub use queue::{BasicEventQueue, ConcurrentEventQueue, EventQueue, FmtPriority, PerRouterQueue};
#[cfg(feature = "rand_queue")]
mod rand_queue;
#[cfg(feature = "rand_queue")]
pub use rand_queue::{GeoTimingModel, ModelParams, SimpleTimingModel};

use crate::{
    bgp::BgpEvent,
    ospf::{local::OspfEvent, OspfArea},
    scion::{event::ScionEvent, types::IsdAs},
    types::{IntoIpv4Prefix, Ipv4Prefix, Prefix, RouterId, StepUpdate},
};

/// Event to handle
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(
    serialize = "P: Serialize, T: serde::Serialize",
    deserialize = "P: for<'a> serde::Deserialize<'a>, T: for<'a> serde::Deserialize<'a>"
))]
pub enum Event<P: Prefix, T> {
    /// BGP Event from `#1` to `#2`.
    Bgp {
        /// The priority (time). Can be ignored when handling events, (unless you implement a custom
        /// queue).
        p: T,
        /// The source of the message
        src: RouterId,
        /// The target of the message
        dst: RouterId,
        /// The specific BGP event.
        e: BgpEvent<P>,
    },
    /// OSPF Event from directed towards `#1` from `#2.source()`.
    Ospf {
        /// The priority (time). Can be ignored when handling events, (unless you implement a custom
        /// queue).
        p: T,
        /// The source of the message
        src: RouterId,
        /// The target of the message
        dst: RouterId,
        /// The OSPF area that the message carries
        area: OspfArea,
        /// The specific OSPF event.
        e: OspfEvent,
    },
    /// SCION Event from AS `src` to AS `dst` (AS-level, not router-level).
    ///
    /// SCION events operate at the AS level for scalability. Multiple routers
    /// within an AS share a single ScionControlService that handles beaconing
    /// and path segment registration.
    ///
    /// Note: Serialization is not supported for SCION events due to Arc-based data structures.
    #[serde(skip)]
    Scion {
        /// The priority (time). Can be ignored when handling events, (unless you implement a custom
        /// queue).
        p: T,
        /// The source ISD-AS
        src: IsdAs,
        /// The destination ISD-AS
        dst: IsdAs,
        /// The specific SCION event.
        e: ScionEvent,
    },
}

impl<P: Prefix, T> Event<P, T> {
    /// Create a new BGP event
    pub fn bgp(p: T, src: RouterId, dst: RouterId, e: BgpEvent<P>) -> Self {
        Self::Bgp { p, src, dst, e }
    }

    /// Create a new OSPF event
    pub fn ospf(p: T, src: RouterId, dst: RouterId, area: OspfArea, e: OspfEvent) -> Self {
        Self::Ospf {
            p,
            src,
            dst,
            area,
            e,
        }
    }

    /// Create a new SCION event
    pub fn scion(p: T, src: IsdAs, dst: IsdAs, e: ScionEvent) -> Self {
        Self::Scion { p, src, dst, e }
    }

    /// Returns the prefix for which this event talks about.
    pub fn prefix(&self) -> Option<P> {
        match self {
            Event::Bgp {
                e: BgpEvent::Update(route),
                ..
            } => Some(route.prefix),
            Event::Bgp {
                e: BgpEvent::Withdraw(prefix),
                ..
            } => Some(*prefix),
            Event::Ospf { .. } | Event::Scion { .. } => None,
        }
    }

    /// Get a reference to the priority of this event.
    pub fn priority(&self) -> &T {
        match self {
            Event::Bgp { p, .. } | Event::Ospf { p, .. } | Event::Scion { p, .. } => p,
        }
    }

    /// Get a reference to the priority of this event.
    pub fn priority_mut(&mut self) -> &mut T {
        match self {
            Event::Bgp { p, .. } | Event::Ospf { p, .. } | Event::Scion { p, .. } => p,
        }
    }

    /// Returns true if the event is a bgp message
    pub fn is_bgp_event(&self) -> bool {
        matches!(self, Event::Bgp { .. })
    }

    /// Returns true if the event is a scion message
    pub fn is_scion_event(&self) -> bool {
        matches!(self, Event::Scion { .. })
    }

    /// Return the source of the event (router ID for BGP/OSPF, None for SCION).
    ///
    /// For SCION events, use `scion_source()` to get the IsdAs.
    pub fn source(&self) -> RouterId {
        match self {
            Event::Bgp { src, .. } | Event::Ospf { src, .. } => *src,
            Event::Scion { .. } => RouterId::from(0),  // SCION uses AS-level addressing
        }
    }

    /// Return the SCION source AS (None for BGP/OSPF events)
    pub fn scion_source(&self) -> Option<IsdAs> {
        match self {
            Event::Scion { src, .. } => Some(*src),
            _ => None,
        }
    }

    /// Return the SCION destination AS (None for BGP/OSPF events)
    pub fn scion_destination(&self) -> Option<IsdAs> {
        match self {
            Event::Scion { dst, .. } => Some(*dst),
            _ => None,
        }
    }

    /// Return the router where the event is processed (None for SCION events)
    pub fn router(&self) -> RouterId {
        match self {
            Event::Bgp { dst, .. } | Event::Ospf { dst, .. } => *dst,
            Event::Scion { .. } => RouterId::from(0),  // SCION uses AS-level addressing
        }
    }
}

impl<P: Prefix, T> IntoIpv4Prefix for Event<P, T> {
    type T = Event<Ipv4Prefix, ()>;

    fn into_ipv4_prefix(self) -> Self::T {
        match self {
            Event::Bgp { src, dst, e, .. } => Event::Bgp {
                p: (),
                src,
                dst,
                e: match e {
                    BgpEvent::Withdraw(p) => BgpEvent::Withdraw(p.into_ipv4_prefix()),
                    BgpEvent::Update(bgp_route) => BgpEvent::Update(bgp_route.into_ipv4_prefix()),
                },
            },
            Event::Ospf {
                src, dst, area, e, ..
            } => Event::Ospf {
                p: (),
                src,
                dst,
                area,
                e,
            },
            Event::Scion { src, dst, e, .. } => Event::Scion {
                p: (),
                src,
                dst,
                e,
            },
        }
    }
}

// Manual trait implementations for Event
// Scion events use Arc, so we compare by pointer equality

impl<P: Prefix, T: PartialEq> PartialEq for Event<P, T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Event::Bgp { p: p1, src: src1, dst: dst1, e: e1 },
             Event::Bgp { p: p2, src: src2, dst: dst2, e: e2 }) => {
                p1 == p2 && src1 == src2 && dst1 == dst2 && e1 == e2
            }
            (Event::Ospf { p: p1, src: src1, dst: dst1, area: area1, e: e1 },
             Event::Ospf { p: p2, src: src2, dst: dst2, area: area2, e: e2 }) => {
                p1 == p2 && src1 == src2 && dst1 == dst2 && area1 == area2 && e1 == e2
            }
            (Event::Scion { .. }, Event::Scion { .. }) => {
                // SCION events with Arc are never equal (pointer comparison would be needed)
                false
            }
            _ => false,
        }
    }
}

impl<P: Prefix, T: Eq> Eq for Event<P, T> {}

impl<P: Prefix, T: std::hash::Hash> std::hash::Hash for Event<P, T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Event::Bgp { p, src, dst, e } => {
                0u8.hash(state);
                p.hash(state);
                src.hash(state);
                dst.hash(state);
                e.hash(state);
            }
            Event::Ospf { p, src, dst, area, e } => {
                1u8.hash(state);
                p.hash(state);
                src.hash(state);
                dst.hash(state);
                area.hash(state);
                e.hash(state);
            }
            Event::Scion { p, src, dst, .. } => {
                // Hash only the metadata, not the Arc contents
                2u8.hash(state);
                p.hash(state);
                src.hash(state);
                dst.hash(state);
            }
        }
    }
}

/// The outcome of a handled event. This will include a update in the forwarding state (0:
/// [`StepUpdate`]), and a set of new events that must be enqueued (1: [`Event`]).
pub(crate) type EventOutcome<P, T> = (StepUpdate<P>, Vec<Event<P, T>>);
