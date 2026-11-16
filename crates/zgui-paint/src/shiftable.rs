//! Whether a scrollport's pixels may be moved instead of drawn again.
//!
//! A renderer that keeps its composed frame can answer a scroll by translating the part of the port
//! that is still valid. What it cannot do is decide whether that is the same picture, because the
//! two things that would make it a different picture are properties of the document:
//!
//! * **something else draws in the port.** A tooltip over a list, a dialog, a sibling that overlaps
//!   it — the composed pixels are the composite, so moving them moves whatever was composited in.
//! * **the port is not opaque.** The composite includes whatever showed through the content, and
//!   what showed through does not scroll. Moving it smears it.
//!
//! Both are refusals rather than corrections. The fallback is the frame the window would have drawn
//! anyway, so a refusal costs the optimisation and never the picture.
//!
//! # What is not tested here, because it cannot happen
//!
//! That the movement was rigid. The fragment pass reports a move only for a scroll container that
//! draws its content in a space of its own, and it reports one move for the whole port. A sticky
//! or fixed box below it does not move with the content, and the fragment pass damages it beyond
//! the move: where the moved pixels put it, and where it is now. A container that keeps its
//! children in its own space reports no move, so the frame draws its port again.

use zgui_css::ComputedStyle;
use zgui_css::values::image::ImageValue;
use zgui_dom::NodeKey;
use zgui_dom::side::BoxKey;
use zgui_geom::{Device, DevicePx, Rect, Size};
use zgui_layout::FragmentKind;
use zgui_layout::LayoutStore;
use zgui_layout::scroll_region;

use crate::walk::stacking::children_in_paint_order;

/// Why a port's pixels may not be moved.
///
/// Reported rather than folded into a bool so that a counter can say which refusal a document keeps
/// hitting: "scrolling is not being shifted" and "scrolling is not being shifted *because the list
/// has no background*" are different bugs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The element does not generate a box, or generates no scrollable region.
    NotAScroller,
    /// Its background lets what is behind it through, so the composite is not the content's alone.
    /// Carries the element that marks the port under the content, when one does.
    NotOpaque(Option<NodeKey>),
    /// Something outside the container draws inside the port. Carries the element that does.
    Overdrawn(Option<NodeKey>),
}

/// A scrollport whose pixels may be moved, and what in it does not move with them.
#[derive(Clone, Debug, PartialEq)]
pub struct Shiftable {
    /// The port, in device pixels.
    pub port: Rect<DevicePx, Device>,
    /// What marks pixels in the port without scrolling: drawn over the content (an overlay
    /// scrollbar) or between it and its backing (a glow along a panel's top). The caller may move
    /// the part of the port clear of all of them and draw the rest again, or refuse.
    pub apart: Vec<Apart>,
}

/// Something marking pixels in a port that does not scroll with it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Apart {
    /// Its subtree's ink inside the port. The whole subtree's and not its marks', so that a part
    /// of it that moves or resizes this frame stays inside.
    pub ink: Rect<DevicePx, Device>,
    /// The first element found marking a pixel there.
    pub by: Option<NodeKey>,
}

impl Apart {
    /// `key`'s subtree, found marking the port as `by`.
    fn of(device: OnDevice<'_>, key: BoxKey, port: Pixels, by: Option<NodeKey>) -> Self {
        let ink = subtree_ink(device, key)
            .and_then(|ink| ink.intersection(port))
            .unwrap_or(port);
        Self { ink, by }
    }
}

/// Whether `container`'s scrollport may have its pixels moved, and the port if it may.
///
/// The port is in device pixels, in the same space the damage set is measured in: `placements`
/// puts it there. The tests below compare it against other fragments on the device too, because
/// fragments outside the container can measure their rectangles in other anchors.
pub fn port_may_be_shifted(
    store: &LayoutStore,
    container: NodeKey,
    placements: &zgui_scene::Placements,
    scale: f32,
) -> Result<Shiftable, Refusal> {
    let Some(&box_key) = store.boxes_of(container).first() else {
        return Err(Refusal::NotAScroller);
    };
    if scroll_region::region_of(store, box_key).is_none() {
        return Err(Refusal::NotAScroller);
    }
    let Some(port) = scrollport_of(store, box_key) else {
        return Err(Refusal::NotAScroller);
    };
    let piece = store
        .fragments_of_box(box_key)
        .first()
        .and_then(|frag| store.fragment(*frag))
        .ok_or(Refusal::NotAScroller)?;
    // The port is the padding box, which is measured in the space the box is laid out in, as
    // every border box the tests below read.
    let port =
        zgui_layout::fragment::transform::placed::onto_device(port, piece.transform, placements);
    let device = OnDevice {
        store,
        placements,
        scale,
    };
    // Two independent conditions, and neither implies the other. A port with an opaque background
    // of its own still cannot be moved if a dialog is drawn over it, and a port with nothing over
    // it still cannot be moved if what shows through it is a gradient.
    let mut apart = Vec::new();
    drawn_over(device, box_key, port, &mut apart).map_err(|()| Refusal::Overdrawn(None))?;
    backing(device, box_key, port, &mut apart)?;
    Ok(Shiftable { port, apart })
}

/// The store, and the matrices that put its fragments on the device.
#[derive(Clone, Copy)]
struct OnDevice<'a> {
    /// The boxes and their fragments.
    store: &'a LayoutStore,
    /// The matrices of the drawn frame.
    placements: &'a zgui_scene::Placements,
    /// Device pixels per CSS pixel, which a corner radius is resolved with.
    scale: f32,
}

/// Where the pixels `box_key` scrolls stand, in the space the container is laid out in.
///
/// A scroll region reports its port at the origin, because that is the space its offsets are
/// measured in. Every test below compares the port against fragments on the device. So the port
/// is taken from the container's own fragment: the padding box, less the scrollbar gutter on
/// the sides the layout engine took it off. A port read at the origin names the pixels at the top
/// left of the window, which only coincide with the list's when the list stands there.
fn scrollport_of(store: &LayoutStore, box_key: BoxKey) -> Option<Rect<DevicePx, Device>> {
    let layout = store.layout_of(box_key)?;
    let piece = store.fragment(*store.fragments_of_box(box_key).first()?)?;
    let padding = piece.padding_box();
    let width = (padding.size.width.0 - layout.scrollbar_size.width.0).max(0.0);
    let height = (padding.size.height.0 - layout.scrollbar_size.height.0).max(0.0);
    Some(Rect::new(
        padding.origin,
        Size::new(DevicePx(width), DevicePx(height)),
    ))
}

/// Whether nothing painted *after* `box_key` puts ink inside `port`.
///
/// Walks from the box up to the root, and at each level asks which of that level's children paint
/// after the one leading down to the container. Only those can be over it; anything earlier is
/// behind it and is [`backing`]'s question rather than this one's.
///
/// A later sibling whose whole folded subtree misses the port is dismissed without being entered,
/// so the cost is the container's depth and the fan-out along it rather than the document.
///
/// Adds what does to `apart`, and is `Err` when the walk lost its way.
fn drawn_over(
    device: OnDevice<'_>,
    box_key: BoxKey,
    port: Pixels,
    apart: &mut Vec<Apart>,
) -> Result<(), ()> {
    let store = device.store;
    let root = store.root().ok_or(())?;
    let mut inside = box_key;
    while inside != root {
        let parent = store.get(inside).and_then(|node| node.parent).ok_or(())?;
        let order = children_in_paint_order(store, parent);
        let position = order.iter().position(|&child| child == inside).ok_or(())?;
        for &later in &order[position + 1..] {
            if let Some(by) = paints_in(device, later, port) {
                apart.push(Apart::of(device, later, port, by));
            }
        }
        inside = parent;
    }
    Ok(())
}

/// Whether what the port is composited over is a uniform colour that nothing draws across.
///
/// The composed pixels inside a scrollport are the composite of everything under it, and moving
/// them moves all of it. That is the same picture on one condition: everything the content was
/// composited *over* is one flat colour, so translating it is the identity, and nothing was
/// composited *between* that colour and the content.
///
/// It is deliberately not "the scroll container has a background". A list usually does not — it
/// inherits the page's — and requiring one would refuse the ordinary case while accepting a
/// contrived one. What is required is that somewhere at or above the container there is an opaque
/// solid colour covering the port, and that between it and the content nothing else marks a pixel
/// inside the port.
///
/// The walk is up the ancestors, and at every level it is the same three questions, so it costs the
/// container's depth and the fan-out along it — never the document.
///
/// Siblings that mark the port between the backing and the content are added to `apart` rather
/// than refused: the part of the port clear of them is still the content over a flat colour.
fn backing(
    device: OnDevice<'_>,
    box_key: BoxKey,
    port: Pixels,
    apart: &mut Vec<Apart>,
) -> Result<(), Refusal> {
    let store = device.store;
    let Some(root) = store.root() else {
        return Err(Refusal::NotAScroller);
    };

    // The container's own background comes first: one that is opaque and flat hides everything
    // behind it, and then nothing above matters at all.
    if flat_and_opaque_over(device, box_key, port) {
        return Ok(());
    }

    let mut inside = box_key;
    loop {
        let Some(parent) = store.get(inside).and_then(|node| node.parent) else {
            // Not under the root this frame, which is not a document anything should be moving
            // pixels for.
            return Err(Refusal::Overdrawn(None));
        };

        // Every sibling at this level that paints *before* the one leading down to the container.
        // It is painted over whatever backs it and under the content, so it is between them, and it
        // does not scroll. What paints after is the other condition's to refuse.
        let order = children_in_paint_order(store, parent);
        let position = order.iter().position(|&child| child == inside);
        for &sibling in &order[..position.unwrap_or(0)] {
            if let Some(by) = paints_in(device, sibling, port) {
                apart.push(Apart::of(device, sibling, port, by));
            }
        }

        // The parent's own painting is behind everything at this level. Flat and opaque over the
        // whole port, it *is* the backing and the walk is done.
        if flat_and_opaque_over(device, parent, port) {
            return Ok(());
        }
        // A flat colour that is not opaque, across the whole port, is a uniform layer: what it is
        // composited over shows through it, but as long as that is uniform too, the composite is.
        // A translucent panel (one a compositor blurs behind) is the ordinary case.
        let layered = flat_over(device, parent, port);
        // Anything else it marks inside the port is between whatever backs it and the content.
        if !layered && marks_pixels_in(device, parent, port) {
            return Err(Refusal::NotOpaque(node_of(store, parent)));
        }
        if parent == root {
            // Nothing at or below the root marked these pixels but flat layers. Behind them is what
            // the renderer clears the window to, one colour across the surface, so the composite
            // over it is uniform too.
            return Ok(());
        }
        inside = parent;
    }
}

/// Whether `key` paints one flat opaque colour across the whole of `port`.
///
/// Flat because a gradient or an image is not the same picture when it is translated, and opaque
/// because anything less lets what is behind it into the composite. Across the whole port, because
/// a colour that covers half of it backs half of it.
fn flat_and_opaque_over(device: OnDevice<'_>, key: BoxKey, port: Pixels) -> bool {
    let store = device.store;
    let Some(style) = store.get(key).map(|node| &node.style) else {
        return false;
    };
    if !paints_opaquely(style) {
        return false;
    }
    // The background is painted over the border box, so that is what has to contain the port.
    store
        .fragments_of_box(key)
        .first()
        .and_then(|frag| store.fragment(*frag))
        .is_some_and(|piece| {
            let border_box = zgui_layout::fragment::transform::placed::onto_device(
                piece.border_box,
                piece.transform,
                device.placements,
            );
            border_box.intersection(port) == Some(port)
        })
}

/// Whether `key` paints one flat colour, opaque or not, across the whole of `port`, and nothing
/// else of its own there.
///
/// Its rounded corners stay clear of the port, so the colour is the same at every pixel the port
/// holds; its border marks none of them; it has no shadow or outline.
fn flat_over(device: OnDevice<'_>, key: BoxKey, port: Pixels) -> bool {
    let store = device.store;
    let Some(style) = store.get(key).map(|node| &node.style) else {
        return false;
    };
    let background = style.get_background();
    if background
        .background_image
        .0
        .iter()
        .any(|image| !matches!(image, ImageValue::None))
    {
        return false;
    }
    if crate::lower::outline::of(style, 1.0).is_some()
        || !style.get_effects().box_shadow.0.is_empty()
    {
        return false;
    }
    let Some(piece) = store
        .fragments_of_box(key)
        .first()
        .and_then(|frag| store.fragment(*frag))
    else {
        return false;
    };
    let on_device = |rect| {
        zgui_layout::fragment::transform::placed::onto_device(
            rect,
            piece.transform,
            device.placements,
        )
    };
    let border_box = on_device(piece.border_box);
    if border_box.intersection(port) != Some(port) {
        return false;
    }
    let ring = !crate::lower::border::of(style).invisible;
    let inner = if ring { on_device(piece.padding_box()) } else { border_box };
    inner.intersection(port) == Some(port) && clear_of_corners(device, style, piece, port)
}

/// Whether `port` stays out of the squares `piece`'s rounded corners cut from its border box,
/// where neither the background nor the inside of the border is the same as along the sides.
fn clear_of_corners(
    device: OnDevice<'_>,
    style: &ComputedStyle,
    piece: &zgui_layout::Fragment,
    port: Pixels,
) -> bool {
    let radii = zgui_layout::fragment::clip::radii(style, piece.border_box, device.scale);
    let reach = [radii.top_left, radii.top_right, radii.bottom_right, radii.bottom_left]
        .iter()
        .fold(0.0_f32, |held, corner| held.max(corner.x.0).max(corner.y.0));
    if reach <= 0.0 {
        return true;
    }
    let border_box = zgui_layout::fragment::transform::placed::onto_device(
        piece.border_box,
        piece.transform,
        device.placements,
    );
    let (left, top) = (border_box.origin.x.0, border_box.origin.y.0);
    let (right, bottom) = (
        left + border_box.size.width.0,
        top + border_box.size.height.0,
    );
    let square = |x: f32, y: f32| {
        Rect::new(
            zgui_geom::Point::new(DevicePx(x), DevicePx(y)),
            Size::new(DevicePx(reach), DevicePx(reach)),
        )
    };
    [
        square(left, top),
        square(right - reach, top),
        square(left, bottom - reach),
        square(right - reach, bottom - reach),
    ]
    .iter()
    .all(|corner| corner.intersection(port).is_none())
}

/// Whether `piece`, a fragment of `key`, marks a pixel inside `port`.
///
/// Its ink is its whole extent, which for a box that paints only a border (a hairline ring around
/// a panel) is a rectangle around pixels it never touches: such a box marks the port only where
/// its border band, or a corner its radius rounds, reaches into it.
fn marks_in(device: OnDevice<'_>, key: BoxKey, piece: &zgui_layout::Fragment, port: Pixels) -> bool {
    let store = device.store;
    if piece
        .device_ink(device.placements)
        .intersection(port)
        .is_none()
        || !marks_pixels(store, key, piece.kind)
    {
        return false;
    }
    if !matches!(piece.kind, FragmentKind::Box) {
        return true;
    }
    let Some(style) = store.get(key).map(|node| &node.style) else {
        return true;
    };
    let background = crate::lower::background::of(style);
    let only_a_border = background.color.alpha() <= 0.0
        && background.layers.is_empty()
        && crate::lower::outline::of(style, 1.0).is_none()
        && style.get_effects().box_shadow.0.is_empty();
    if !only_a_border {
        return true;
    }
    let inner = zgui_layout::fragment::transform::placed::onto_device(
        piece.padding_box(),
        piece.transform,
        device.placements,
    );
    !(inner.intersection(port) == Some(port) && clear_of_corners(device, style, piece, port))
}

/// Whether a box's own background hides everything behind it, with one flat colour.
///
/// A solid, fully opaque colour is the one case where moving a composite is exactly moving the
/// content drawn on it: the colour is uniform, so translating it is the identity, and nothing
/// behind it contributed a pixel. A gradient or an image is not uniform and would smear; a
/// translucent colour lets what is behind it through, and what is behind it did not scroll.
fn paints_opaquely(style: &ComputedStyle) -> bool {
    let background = style.get_background();
    // Not `is_empty`: the initial value of `background-image` is a one-element list holding `none`,
    // so an emptiness test refuses every box in every document.
    if background
        .background_image
        .0
        .iter()
        .any(|image| !matches!(image, ImageValue::None))
    {
        return false;
    }
    // `currentColor` resolves against the box's own colour, which is already absolute here.
    let colour = background
        .background_color
        .resolve_to_absolute(&style.get_inherited_text().clone_color());
    colour.alpha >= 1.0
}

/// Whether `key`'s own fragments mark a pixel inside `port`, ignoring its descendants.
fn marks_pixels_in(device: OnDevice<'_>, key: BoxKey, port: Pixels) -> bool {
    let store = device.store;
    store.fragments_of_box(key).iter().any(|&frag| {
        store
            .fragment(frag)
            .is_some_and(|piece| marks_in(device, key, piece, port))
    })
}

/// Whether anything in `key`'s subtree actually marks a pixel inside `port`.
///
/// The folded subtree ink is the *refusal* test and not the answer: a container whose descendants
/// straddle the port but whose painted boxes all avoid it marks nothing there, and taking the union
/// as the answer refuses every document with a full-height sibling in it. So the union is used the
/// way the emit walk uses it — to dismiss a whole subtree in constant time — and the question is
/// then asked of each box's own ink. Answers with the element of the first box that does.
fn paints_in(device: OnDevice<'_>, key: BoxKey, port: Pixels) -> Option<Option<NodeKey>> {
    let store = device.store;
    let folded = subtree_ink(device, key)?;
    folded.intersection(port)?;
    for &frag in store.fragments_of_box(key) {
        let Some(piece) = store.fragment(frag) else {
            continue;
        };
        if marks_in(device, key, piece, port) {
            return Some(piece.node);
        }
    }
    store
        .get(key)?
        .children
        .iter()
        .find_map(|&child| paints_in(device, child, port))
}

/// The element `key` is a box of, for a refusal to name.
fn node_of(store: &LayoutStore, key: BoxKey) -> Option<NodeKey> {
    let frag = *store.fragments_of_box(key).first()?;
    store.fragment(frag)?.node
}

/// Whether a fragment of this box actually marks a pixel, rather than merely occupying a rectangle.
///
/// A fragment's ink is its geometric extent, so a full-height wrapper with no background "inks" the
/// whole window while drawing nothing at all. Asking the extent alone therefore refuses every
/// document with a layout wrapper in it, which is every document.
///
/// The lowerings are the paint stage's own, so this cannot drift from what is actually drawn: if
/// something new starts marking pixels, it starts marking them here too.
fn marks_pixels(store: &LayoutStore, key: BoxKey, kind: FragmentKind) -> bool {
    // Anything that is not a plain box is content: a run of text, a replaced element, a drawing, a
    // scrollbar. All of them draw.
    if !matches!(kind, FragmentKind::Box) {
        return true;
    }
    let Some(style) = store.get(key).map(|node| &node.style) else {
        return false;
    };
    let background = crate::lower::background::of(style);
    if background.color.alpha() > 0.0 || !background.layers.is_empty() {
        return true;
    }
    if !crate::lower::border::of(style).invisible {
        return true;
    }
    if crate::lower::outline::of(style, 1.0).is_some() {
        return true;
    }
    !style.get_effects().box_shadow.0.is_empty()
}

/// The union of a box's ink and everything below it, as the last fragment pass folded it, on the
/// device.
fn subtree_ink(device: OnDevice<'_>, key: BoxKey) -> Option<Pixels> {
    let store = device.store;
    let frag = *store.fragments_of_box(key).first()?;
    store
        .fragment(frag)
        .map(|fragment| fragment.device_subtree_ink(device.placements))
}

/// A rectangle in device pixels.
type Pixels = Rect<DevicePx, Device>;
