//! Blur behind a surface, where the compositor offers it.

use std::sync::OnceLock;

use smithay_client_toolkit::globals::GlobalData;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_protocols::ext::background_effect::v1::client::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1;
use wayland_protocols::ext::background_effect::v1::client::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1;

use crate::driver::WaylandState;

/// A surface's background effect: made on the first request for one, since most never ask.
#[derive(Debug, Default)]
pub struct Effect {
    /// `ext_background_effect_manager_v1`, when the compositor has it.
    manager: Option<ExtBackgroundEffectManagerV1>,
    /// This surface's object.
    object: OnceLock<ExtBackgroundEffectSurfaceV1>,
}

impl Effect {
    /// An effect to be made against `manager`.
    pub const fn new(manager: Option<ExtBackgroundEffectManagerV1>) -> Self {
        Self {
            manager,
            object: OnceLock::new(),
        }
    }

    /// The surface's effect object, or nothing where the compositor has no such protocol.
    pub fn object(
        &self,
        qh: &QueueHandle<WaylandState>,
        surface: &WlSurface,
    ) -> Option<&ExtBackgroundEffectSurfaceV1> {
        let manager = self.manager.as_ref()?;
        Some(
            self.object
                .get_or_init(|| manager.get_background_effect(surface, qh, ())),
        )
    }

    /// Releases the object, before the surface it belongs to goes.
    pub fn release(&mut self) {
        if let Some(object) = self.object.take() {
            object.destroy();
        }
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, GlobalData> for WaylandState {
    fn event(
        _state: &mut Self,
        _manager: &ExtBackgroundEffectManagerV1,
        _event: <ExtBackgroundEffectManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Capabilities: a compositor that drops blur simply stops applying the region.
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _effect: &ExtBackgroundEffectSurfaceV1,
        _event: <ExtBackgroundEffectSurfaceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}
