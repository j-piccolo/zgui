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
//!    [`zgui_paint::shiftable`]. Something drawn over one edge of it, an overlay scrollbar, is
//!    answered by moving the rest of the port and drawing that edge's band again.

use zgui_bits::DamageSet;
use zgui_geom::{Device, Point, Rect, Size};
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

/// A scroll answered by moving pixels, and the parts of its port drawn again instead.
#[derive(Clone, Debug)]
pub(crate) struct Shift {
    /// The pixels moved: the part of the port clear of everything in it that does not scroll.
    pub(crate) moved: ScrollShift,
    /// The whole port, `moved`'s and `beside`'s together.
    pub(crate) port: Rect<i32, Device>,
    /// The rest of the port, at most a band along each edge. What does not scroll is drawn there,
    /// so its pixels are owed as damage rather than moved.
    pub(crate) beside: Vec<Rect<i32, Device>>,
    /// What does not scroll in the port, for the trace: the element and its ink.
    pub(crate) apart: Vec<(Option<zgui_dom::NodeKey>, Rect<i32, Device>)>,
}

/// The largest part of `port` clear of every rectangle in `apart`, and the bands around it.
///
/// Each rectangle is cut away by keeping the largest of the four sides of it, so a scrollbar along
/// the right edge costs the port a column and a glow along the top costs it a row. `None` when what
/// is left is less than half the port: something in the middle of it, not along an edge.
fn clear_of(
    port: Rect<i32, Device>,
    apart: impl IntoIterator<Item = Rect<i32, Device>>,
) -> Option<(Rect<i32, Device>, Vec<Rect<i32, Device>>)> {
    let edges = |r: Rect<i32, Device>| {
        (
            r.origin.x,
            r.origin.y,
            r.origin.x + r.size.width,
            r.origin.y + r.size.height,
        )
    };
    let rect = |l: i32, t: i32, r: i32, b: i32| {
        Rect::new(Point::new(l, t), Size::new((r - l).max(0), (b - t).max(0)))
    };
    let area = |r: Rect<i32, Device>| i64::from(r.size.width) * i64::from(r.size.height);
    let mut kept = port;
    for ink in apart {
        let Some(ink) = ink.intersection(kept) else {
            continue;
        };
        let (l, t, r, b) = edges(kept);
        let (il, it, ir, ib) = edges(ink);
        kept = [
            rect(l, t, il, b),
            rect(ir, t, r, b),
            rect(l, t, r, it),
            rect(l, ib, r, b),
        ]
        .into_iter()
        .max_by_key(|side| area(*side))?;
    }
    if area(kept) * 2 < area(port) {
        return None;
    }
    let (l, t, r, b) = edges(port);
    let (kl, kt, kr, kb) = edges(kept);
    let beside = [
        rect(l, t, r, kt),
        rect(l, kb, r, b),
        rect(l, kt, kl, kb),
        rect(kr, kt, r, kb),
    ]
    .into_iter()
    .filter(|band| !band.is_empty())
    .collect();
    Some((kept, beside))
}

/// `damage` with every rectangle that runs across most of `port` widened to run across all of it.
///
/// A shifted frame's damage is strips along the scroll's edges: the band the move exposes, rows
/// whose look changes as they reach an edge, the band of a glow the port is not moved under. Each
/// is a little narrower or wider than the next (a row stops short of the scrollbar, a glow runs
/// to the panel's edge), so the set keeps them apart, and every rectangle is a replay of the draw
/// list and a copy to the surface of its own. Run to the port's sides they merge, at the cost of
/// the few pixels between.
pub(crate) fn across(damage: &DamageSet, port: Rect<i32, Device>, by: (i32, i32)) -> DamageSet {
    let mut tidy = DamageSet::new();
    for &rect in damage.rects() {
        let Some(inside) = rect.intersection(port) else {
            tidy.absorb(rect);
            continue;
        };
        let widened = if by.0 == 0 && 2 * inside.size.width >= port.size.width {
            rect.union(Rect::new(
                Point::new(port.origin.x, inside.origin.y),
                Size::new(port.size.width, inside.size.height),
            ))
        } else if by.1 == 0 && 2 * inside.size.height >= port.size.height {
            rect.union(Rect::new(
                Point::new(inside.origin.x, port.origin.y),
                Size::new(inside.size.width, port.size.height),
            ))
        } else {
            rect
        };
        tidy.absorb(widened);
    }
    tidy
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

impl Refused {
    /// The element the document named in refusing, when it named one.
    pub(crate) fn element(self) -> Option<zgui_dom::NodeKey> {
        match self {
            Self::Document(
                shiftable::Refusal::Overdrawn(node) | shiftable::Refusal::NotOpaque(node),
            ) => node,
            _ => None,
        }
    }
}

impl Window {
    /// This frame's scroll as a movement of pixels, when it is one.
    ///
    /// `viewport` is the surface's extent, which the port is cut to: nothing outside the surface is
    /// in the composed target to be moved.
    pub(crate) fn scroll_shift(
        &self,
        viewport: zgui_geom::Size<i32, Device>,
    ) -> Result<Shift, Refused> {
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

        let shiftable = {
            let layout = self.layout.borrow();
            shiftable::port_may_be_shifted(
                &layout,
                scrolled.container,
                &self.host.placements(),
                self.scale,
            )
            .map_err(Refused::Document)?
        };
        let surface = Rect::new(Point::new(0, 0), viewport);
        let pixels = zgui_layout::fragment::diff::pixels;
        let Some(port) = pixels(shiftable.port).intersection(surface) else {
            return Err(Refused::NotOneContainer);
        };
        let by = (dx as i32, dy as i32);
        let refused = Refused::Document(shiftable::Refusal::Overdrawn(
            shiftable.apart.first().and_then(|apart| apart.by),
        ));
        let whole = port;
        // Only for a trace: an untraced frame allocates nothing for it.
        let apart = if zgui_profile::latency::listening() {
            shiftable
                .apart
                .iter()
                .map(|apart| (apart.by, pixels(apart.ink)))
                .collect()
        } else {
            Vec::new()
        };
        let (port, beside) =
            clear_of(port, shiftable.apart.iter().map(|apart| pixels(apart.ink))).ok_or(refused)?;
        Ok(Shift {
            moved: ScrollShift { port, by },
            port: whole,
            beside,
            apart,
        })
    }
}
