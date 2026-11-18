//! What a coordinate system's subtree covers, in that coordinate system's own terms.

use zgui_geom::{Device, DevicePx, Point, Rect, transformed_bounds};

use crate::prim::PrimitiveKind;
use crate::scene::Scene;
use crate::spatial::SpatialId;

/// Everything drawn through `node` or through anything below it, in `node`'s own coordinates.
///
/// Read in `node`'s space rather than the device's, because that is the half of the answer that
/// does *not* move when the node does: the same rectangle, mapped through the matrix before the
/// write and through the matrix after it, is where the ink was and where it is. Asking twice in
/// device space would need the whole walk twice.
///
/// `None` when nothing is drawn through the subtree at all, which is a real answer and not an
/// empty one — there is no rectangle to damage.
pub fn under(scene: &Scene, node: SpatialId) -> Option<Rect<DevicePx, Device>> {
    let mut union: Option<Rect<DevicePx, Device>> = None;
    for op in scene.ops() {
        // A group marker's rectangle is its contents', which are counted through their own
        // primitives; a backdrop reads the target in the device's own coordinates and names no
        // coordinate system at all.
        if matches!(
            op.kind,
            PrimitiveKind::GroupStart | PrimitiveKind::GroupEnd | PrimitiveKind::Backdrop
        ) {
            continue;
        }
        let Some(space) = scene.space_of_op(*op) else {
            continue;
        };
        let Some(into) = scene.spatial.relative(space, node) else {
            continue;
        };
        let ink = transformed_bounds(&into, scene.ink_of(*op));
        union = Some(match union {
            Some(held) => held.union(ink),
            None => ink,
        });
    }
    union
}

/// Where everything drawn through `node` or below it lands on the device now, each primitive cut
/// to its clip chain.
///
/// The damage of a move, asked once before the write and once after it. [`under`] is not cut,
/// because a region to travel through has to hold the whole subtree, but its damage would be: a
/// scrolled list holds rows its port hides, and they would all be drawn again.
pub fn on_device(scene: &Scene, node: SpatialId) -> Option<Rect<DevicePx, Device>> {
    let resolve = |id| scene.spatial.resolve(id);
    // A list's rows share a handful of chains, and resolving one walks its links' spaces.
    let mut admitted: std::collections::HashMap<crate::ClipId, Rect<DevicePx, Device>> =
        std::collections::HashMap::new();
    let mut spaces: std::collections::HashMap<SpatialId, Option<zgui_geom::Matrix4>> =
        std::collections::HashMap::new();
    let mut union: Option<Rect<DevicePx, Device>> = None;
    for op in scene.ops() {
        if matches!(
            op.kind,
            PrimitiveKind::GroupStart | PrimitiveKind::GroupEnd | PrimitiveKind::Backdrop
        ) {
            continue;
        }
        let Some(space) = scene.space_of_op(*op) else {
            continue;
        };
        let matrix = *spaces.entry(space).or_insert_with(|| {
            scene
                .spatial
                .relative(space, node)
                .and_then(|_| resolve(space))
        });
        // A space outside `node`, or one that resolves to nothing.
        let Some(matrix) = matrix else {
            continue;
        };
        let ink = transformed_bounds(&matrix, scene.ink_of(*op));
        let clip = scene.clip_of_op(*op);
        let bounds = *admitted
            .entry(clip)
            .or_insert_with(|| scene.clips.bounds_placed(clip, &resolve));
        let Some(ink) = ink.intersection(bounds) else {
            continue;
        };
        union = Some(match union {
            Some(held) => held.union(ink),
            None => ink,
        });
    }
    union
}

/// A fractional rectangle grown to the whole device pixels that can show any of it.
///
/// One pixel of slack on every side, because a shape is antialiased against the pixels its edge
/// falls in and a rectangle cut to the edge itself leaves that fringe undrawn.
pub fn whole(bounds: Rect<DevicePx, Device>) -> Rect<i32, Device> {
    if bounds.is_empty() {
        return Rect::ZERO;
    }
    Rect::from_corners(
        Point::new(
            bounds.left().0.floor() as i32 - 1,
            bounds.top().0.floor() as i32 - 1,
        ),
        Point::new(
            bounds.right().0.ceil() as i32 + 1,
            bounds.bottom().0.ceil() as i32 + 1,
        ),
    )
}
