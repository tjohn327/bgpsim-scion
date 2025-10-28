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
use bgpsim::scion::{IsdAs, IsdNumber, ScionAsn};
use bgpsim::types::RouterId;
use std::rc::Rc;
use yew::prelude::*;
use yewdux::prelude::*;

use crate::net::Net;

use super::super::{Divider, Element, TextField, Toggle};

#[cfg(feature = "scion")]
pub struct ScionCfg {
    net: Rc<Net>,
    net_dispatch: Dispatch<Net>,
    isd_input_valid: bool,
    asn_input_valid: bool,
}

#[cfg(feature = "scion")]
pub enum Msg {
    StateNet(Rc<Net>),
    ToggleScionEnabled(bool),
    ToggleIsCore(bool),
    OnIsdChange(String),
    OnScionAsnChange(String),
}

#[cfg(feature = "scion")]
#[derive(Properties, PartialEq, Eq)]
pub struct Properties {
    pub router: RouterId,
    pub disabled: Option<bool>,
}

#[cfg(feature = "scion")]
impl Component for ScionCfg {
    type Message = Msg;
    type Properties = Properties;

    fn create(ctx: &Context<Self>) -> Self {
        let net_dispatch = Dispatch::<Net>::subscribe(ctx.link().callback(Msg::StateNet));
        ScionCfg {
            net: Default::default(),
            net_dispatch,
            isd_input_valid: true,
            asn_input_valid: true,
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let router = ctx.props().router;
        let disabled = ctx.props().disabled.unwrap_or(false);

        let scion_state = self
            .net
            .net()
            .get_router(router)
            .ok()
            .and_then(|r| r.scion().map(|cs| (cs.isd_as, cs.is_core)));

        let scion_enabled = scion_state.is_some();
        let (isd, asn, is_core) = scion_state
            .map(|(isd_as, is_core)| (isd_as.isd, isd_as.asn, is_core))
            .unwrap_or((IsdNumber(1), ScionAsn::new(1).unwrap(), false));

        // Use direct dispatch callbacks to avoid re-entrant borrow issues
        let on_toggle_enabled = self.net_dispatch.reduce_mut_callback(move |n| {
            let enabled = n.net().get_router(router).ok().and_then(|r| r.scion()).is_none();
            if enabled {
                let _ = n.net_mut().enable_scion(router, IsdAs::new(IsdNumber(1), 1u64), false);
            } else {
                let _ = n.net_mut().disable_scion(router);
            }
        });

        let on_toggle_core = self.net_dispatch.reduce_mut_callback(move |n| {
            if let Some((isd_as, is_core)) = n.net().get_router(router).ok().and_then(|r| r.scion().map(|cs| (cs.isd_as, cs.is_core))) {
                let _ = n.net_mut().disable_scion(router);
                let _ = n.net_mut().enable_scion(router, isd_as, !is_core);
            }
        });

        let on_isd_change = ctx.link().callback(Msg::OnIsdChange);
        let on_asn_change = ctx.link().callback(Msg::OnScionAsnChange);

        html! {
            <>
                <Divider text={"SCION Configuration"} />
                <Element text={"SCION Enabled"}>
                    <Toggle text={""} checked={scion_enabled} on_click={on_toggle_enabled} {disabled} />
                </Element>
                {
                    if scion_enabled {
                        html! {
                            <>
                                <Element text={"ISD Number"}>
                                    <TextField
                                        text={format!("{}", isd.0)}
                                        on_change={on_isd_change.clone()}
                                        on_set={on_isd_change}
                                        correct={self.isd_input_valid}
                                        {disabled}
                                    />
                                </Element>
                                <Element text={"AS Number"}>
                                    <TextField
                                        text={format!("{}", asn.as_u64())}
                                        on_change={on_asn_change.clone()}
                                        on_set={on_asn_change}
                                        correct={self.asn_input_valid}
                                        {disabled}
                                    />
                                </Element>
                                <Element text={"Core AS"}>
                                    <Toggle text={""} checked={is_core} on_click={on_toggle_core} {disabled} />
                                </Element>
                            </>
                        }
                    } else {
                        html!()
                    }
                }
            </>
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let router = ctx.props().router;
        match msg {
            Msg::StateNet(n) => {
                self.net = n;
                true
            }
            Msg::ToggleScionEnabled(_) | Msg::ToggleIsCore(_) => {
                // These are now handled directly via reduce_mut_callback in view()
                // This branch should not be reached anymore
                false
            }
            Msg::OnIsdChange(isd_str) => {
                if let Ok(isd_val) = isd_str.parse::<u16>() {
                    self.isd_input_valid = true;
                    // Get current ASN and core status and dispatch the update
                    if let Some((current_isd_as, is_core)) = self
                        .net
                        .net()
                        .get_router(router)
                        .ok()
                        .and_then(|r| r.scion().map(|cs| (cs.isd_as, cs.is_core)))
                    {
                        let new_isd_as = IsdAs::new(IsdNumber(isd_val), current_isd_as.asn);
                        // Use apply instead of reduce_mut to avoid re-entrant borrow
                        self.net_dispatch.apply(move |n| {
                            let mut n = n.clone();
                            let _ = n.net_mut().disable_scion(router);
                            let _ = n.net_mut().enable_scion(router, new_isd_as, is_core);
                            n
                        });
                    }
                } else {
                    self.isd_input_valid = false;
                }
                true
            }
            Msg::OnScionAsnChange(asn_str) => {
                if let Ok(asn_val) = asn_str.parse::<u64>() {
                    if asn_val <= 0xFFFFFFFFFFFF {
                        // Valid 48-bit ASN
                        self.asn_input_valid = true;
                        // Get current ISD and core status and dispatch the update
                        if let Some((current_isd_as, is_core)) = self
                            .net
                            .net()
                            .get_router(router)
                            .ok()
                            .and_then(|r| r.scion().map(|cs| (cs.isd_as, cs.is_core)))
                        {
                            let new_isd_as = IsdAs::new(current_isd_as.isd, asn_val);
                            // Use apply instead of reduce_mut to avoid re-entrant borrow
                            self.net_dispatch.apply(move |n| {
                                let mut n = n.clone();
                                let _ = n.net_mut().disable_scion(router);
                                let _ = n.net_mut().enable_scion(router, new_isd_as, is_core);
                                n
                            });
                        }
                    } else {
                        self.asn_input_valid = false;
                    }
                } else {
                    self.asn_input_valid = false;
                }
                true
            }
        }
    }
}

#[cfg(not(feature = "scion"))]
pub struct ScionCfg;

#[cfg(not(feature = "scion"))]
#[derive(Properties, PartialEq, Eq)]
pub struct Properties {
    pub router: RouterId,
    pub disabled: Option<bool>,
}

#[cfg(not(feature = "scion"))]
impl Component for ScionCfg {
    type Message = ();
    type Properties = Properties;

    fn create(_ctx: &Context<Self>) -> Self {
        ScionCfg
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        html!()
    }
}
