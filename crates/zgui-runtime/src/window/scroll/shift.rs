//! Deciding whether this frame's scroll can be answered by moving pixels that are already drawn.
//!
//! A scroll moves every pixel of a scrollport, so it damages the whole port, so the emit walk
//! reaches every fragment in it and every primitive is pushed and re-ordered again — 77 % of a
//! scroll frame, measured in `docs/perf/scroll-frame.md`. None of that derives anything new. The
//! pixels are already in the renderer's composed target, one whole-pixel translation from where
//! they now belong.
//!
//! This is the gate. It says yes only for a frame that did one thing, and the fallback for every
//! other frame is the frame the window would have drawn anyway — so a refusal costs the saving and
//! never the picture.
//!
//! # The five questions
//!
//! 1. **Can the renderer do it at all?** A renderer without a persistent composed target cannot,
//!    and is never handed a damage set short of what it has to draw.
//! 2. **Did exactly one container move?** Two scrollers gliding at once are two translations, and
//!    one copy answers neither.
//! 3. **Did the walk service the frame incrementally, and did its moves agree?** Anything else the
//!    frame damaged is kept, in `RigidMoves::beyond`, and drawn on top of the moved pixels — which
//!    is what lets a scrollbar thumb move, or a row arrive at the edge, without costing the port.
//! 4. **Was the movement whole pixels, and settled?** A fraction cannot be copied, and an elastic
//!    bounce is deliberately fractional (`Scroller::compose`) so that it does not ratchet.
//! 5. **Is the port the container's alone, and opaque?** Asked of the document, in
//!    [`zgui_paint::shiftable`].

use zgui_bits::DamageSet;
use zgui_geom::{Device, Rect};
use zgui_paint::shiftable;
use zgui_render::ScrollShift;

use crate::window::Window;

/// Why a frame's scroll was not answered by moving pixels.
///
/// Every arm is counted, because "scrolling is not being shifted" and "scrolling is not being
/// shifted *because this list has no background*" are different bugs and only one of them is the
/// framework's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// `ZGUI_SCROLL_SHIFT=0`: every shift refused, so one binary can be measured with and without.
    Off,
    /// The renderer keeps no composed target to move pixels within.
    NoComposedTarget,
    /// Nothing scrolled, or more than one thing did.
    NotOneContainer,
    /// The frame drew something the movement does not account for, or the scroll moved no content
    /// space.
    MoreThanAMove,
    /// The displacement is not a whole number of device pixels, or the container is bouncing.
    NotWholePixels,
    /// The document refused it. Carries which of its reasons.
    Document(shiftable::Refusal),
}

/// Whether shifts are allowed: not when `ZGUI_SCROLL_SHIFT` is `0`.
fn allowed() -> bool {
    static ALLOWED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ALLOWED.get_or_init(|| std::env::var_os("ZGUI_SCROLL_SHIFT").is_none_or(|value| value != "0"))
}

/// What a shifted frame moved and what it drew again, as the trace's `shift.acct` note, in device
/// pixels: the whole `port`; what was `moved` (copied); what was `kept` (moved, and not drawn over
/// by the frame's damage); what was `redrawn` in the port; the frame's damage rectangles, and the
/// damage outside the port. Overlapping damage rectangles count once each, so `redrawn` is an
/// upper bound and `kept` a lower one.
pub(crate) fn account(
    damage: &DamageSet,
    port: Rect<i32, Device>,
    moved: Rect<i32, Device>,
) -> String {
    let area = |rect: Rect<i32, Device>| {
        i64::from(rect.size.width.max(0)) * i64::from(rect.size.height.max(0))
    };
    let inside = |within: Rect<i32, Device>| -> i64 {
        if damage.is_full() {
            return area(within);
        }
        damage
            .rects()
            .iter()
            .filter_map(|rect| rect.intersection(within))
            .map(area)
            .sum::<i64>()
            .min(area(within))
    };
    let redrawn = inside(port);
    let total = damage.area().unwrap_or_else(|| area(port));
    format!(
        "port={} moved={} kept={} redrawn={redrawn} rects={} outside={}",
        area(port),
        area(moved),
        area(moved) - inside(moved),
        damage.rects().len(),
        (total - redrawn).max(0),
    )
}

impl Window {
    /// This frame's scroll as a movement of pixels, when it is one.
    ///
    /// `viewport` is the surface's extent, which the port is cut to: nothing outside the surface is
    /// in the composed target to be moved.
    pub(crate) fn scroll_shift(
        &self,
        viewport: zgui_geom::Size<i32, Device>,
    ) -> Result<ScrollShift, Refused> {
        if !allowed() {
            return Err(Refused::Off);
        }
        if !self.renderer.shifts_composed_pixels() {
            return Err(Refused::NoComposedTarget);
        }
        let [scrolled] = self.scrolled_this_frame.as_slice() else {
            return Err(Refused::NotOneContainer);
        };
        let moves = self.rigid_moves;
        if !moves.settled || moves.conflicted || moves.count == 0 {
            return Err(Refused::MoreThanAMove);
        }

        // Taken from the walk's own report rather than from the two offsets, because the walk is
        // what actually moved the fragments and the two must not be able to disagree.
        let Some((dx, dy)) = moves.by else {
            return Err(Refused::MoreThanAMove);
        };
        if dx.fract() != 0.0 || dy.fract() != 0.0 {
            return Err(Refused::NotWholePixels);
        }
        // An elastic bounce is fractional on purpose and is not finished moving; a container in one
        // is composed against a displacement this frame's whole-pixel copy cannot express.
        if !self
            .scroll
            .borrow()
            .elastic_of(scrolled.container)
            .is_empty()
        {
            return Err(Refused::NotWholePixels);
        }

        let port = {
            let layout = self.layout.borrow();
            shiftable::port_may_be_shifted(&layout, scrolled.container, &self.host.placements())
                .map_err(Refused::Document)?
        };
        let surface = Rect::new(zgui_geom::Point::new(0, 0), viewport);
        let Some(port) = zgui_layout::fragment::diff::pixels(port).intersection(surface) else {
            return Err(Refused::NotOneContainer);
        };
        Ok(ScrollShift {
            port,
            by: (dx as i32, dy as i32),
        })
    }
}
