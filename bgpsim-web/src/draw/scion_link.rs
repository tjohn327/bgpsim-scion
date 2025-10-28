// BgpSim: BGP Network Simulator written in Rust
// Copyright (C) 2022-2023 Tibor Schneider <sctibor@ethz.ch>
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, write to the Free Software Foundation, Inc.,
// 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.

#[cfg(feature = "scion")]
use bgpsim::scion::ScionLinkType;
use bgpsim::types::RouterId;
use yew::prelude::*;
use yewdux::prelude::*;

use crate::{
    draw::SvgColor,
    net::use_pos_pair,
    state::{Hover, State},
};

#[cfg(feature = "scion")]
#[derive(Properties, PartialEq)]
pub struct ScionLinkProps {
    pub src: RouterId,
    pub dst: RouterId,
    pub link_type: ScionLinkType,
}

#[cfg(feature = "scion")]
#[function_component]
pub fn ScionLink(props: &ScionLinkProps) -> Html {
    let (p1, p2) = use_pos_pair(props.src, props.dst);

    // Color and style based on link type
    let (color, stroke_width, dash_array) = match props.link_type {
        ScionLinkType::Core => (SvgColor::BlueLight, "3", ""),
        ScionLinkType::ParentChild => (SvgColor::GreenLight, "2", ""),
        ScionLinkType::Peering => (SvgColor::PurpleLight, "2", "5,5"),
    };

    let state = Dispatch::<State>::new();
    let src = props.src;
    let dst = props.dst;
    let on_mouse_enter = state.reduce_mut_callback(move |s| {
        s.set_hover(Hover::ScionLink(src, dst))
    });
    let on_mouse_leave = state.reduce_mut_callback(|s| s.clear_hover());

    html! {
        <line
            class={classes!("transition-svg", "ease-in-out", color.classes())}
            x1={p1.x().to_string()} y1={p1.y().to_string()}
            x2={p2.x().to_string()} y2={p2.y().to_string()}
            stroke-width={stroke_width}
            stroke-dasharray={dash_array}
            onmouseenter={on_mouse_enter}
            onmouseleave={on_mouse_leave}
        />
    }
}
