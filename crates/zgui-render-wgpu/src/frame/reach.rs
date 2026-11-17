//! The device pixels an instanced batch can write.

use zgui_geom::{Device, DevicePx, Matrix4, Point, Rect};
use zgui_scene::{Batch, ClipId, Paint, PaintKind, PrimitiveKind, Scene, SpatialId};

/// One batch of the frame, and the device pixels it can write.
///
/// `reach` is `None` for a batch that is never culled: every batch that is not instanced, and an
/// instanced batch whose extent cannot be bounded on the device.
#[derive(Clone, Debug, PartialEq)]
pub struct Reached {
    /// The batch.
    pub batch: Batch,
    /// The device pixels it can write, or `None` when it is never culled.
    pub reach: Option<Rect<i32, Device>>,
}

/// The batches of `scene` in draw order, each with the device pixels it can write when `bounded`.
///
/// The merge runs once per frame. A frame of several damage rectangles replays this list once per
/// rectangle, and a batch whose reach misses a rectangle draws nothing there.
pub fn batches(scene: &Scene, bounded: bool) -> Vec<Reached> {
    let mut spaces = Spaces::new(scene);
    scene
        .batches()
        .map(|batch| {
            let reach = if bounded {
                reach_of(scene, &mut spaces, &batch)
            } else {
                None
            };
            Reached { batch, reach }
        })
        .collect()
}

/// The device pixels `batch` can write, or `None` when it is never culled.
///
/// The vertex stage draws a quad, shaded quad, shadow or decoration over its bounds grown by one
/// pixel in its own space, and a sprite over its bounds. This is the larger of the two, put through
/// the instance's matrix the same way, and grown by one more pixel for rounding.
fn reach_of(scene: &Scene, spaces: &mut Spaces<'_>, batch: &Batch) -> Option<Rect<i32, Device>> {
    let (kind, range) = match batch {
        Batch::Quads(range) => (PrimitiveKind::Quad, range),
        Batch::Shaded { range, .. } => (PrimitiveKind::Shaded, range),
        Batch::Shadows(range) => (PrimitiveKind::Shadow, range),
        Batch::Decorations(range) => (PrimitiveKind::Decoration, range),
        Batch::MonoSprites { range, .. } => (PrimitiveKind::MonoSprite, range),
        Batch::SubpixelSprites { range, .. } => (PrimitiveKind::SubpixelSprite, range),
        Batch::ColorSprites { range, .. } => (PrimitiveKind::ColorSprite, range),
        // Each of these changes the target, reads a target or draws out of a payload of its own,
        // so it is planned whatever the scissor is.
        Batch::Marks(_)
        | Batch::Vector(_)
        | Batch::External(_)
        | Batch::Backdrop(_)
        | Batch::Group(_) => return None,
    };
    let remap = scene.remap(kind);
    let mut low = (f32::INFINITY, f32::INFINITY);
    let mut high = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for &slot in remap.get(range.clone())? {
        let (bounds, space) = geometry(scene, kind, slot as usize)?;
        let local = Rect::from_corners(
            Point::new(DevicePx(bounds[0] - 1.0), DevicePx(bounds[1] - 1.0)),
            Point::new(
                DevicePx(bounds[0] + bounds[2] + 1.0),
                DevicePx(bounds[1] + bounds[3] + 1.0),
            ),
        );
        let placed = spaces.on_device(space, local)?;
        low = (low.0.min(placed.left().0), low.1.min(placed.top().0));
        high = (high.0.max(placed.right().0), high.1.max(placed.bottom().0));
    }
    if !(low.0.is_finite() && low.1.is_finite() && high.0.is_finite() && high.1.is_finite()) {
        return None;
    }
    Some(Rect::from_corners(
        Point::new(low.0.floor() as i32 - 1, low.1.floor() as i32 - 1),
        Point::new(high.0.ceil() as i32 + 1, high.1.ceil() as i32 + 1),
    ))
}

/// What one instanced batch covers inside a pass's scissor, for the latency trace: the device
/// pixels its instances cover, counted once per instance, the box around them, and the chunks
/// they came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ink {
    /// Device pixels covered, summed over the instances, so overlap counts each time.
    pub pixels: u64,
    /// The box around every instance's covered part.
    pub covered: Rect<i32, Device>,
    /// The pixels of `pixels` each chunk's instances cover, as `(revision, pixels)`, largest
    /// first. Revision zero is content drawn outside any chunk.
    pub owners: Vec<(u64, u64)>,
}

/// What `batch` covers inside `scissor`, or `None` for a batch that is not instanced or not
/// bounded on the device.
///
/// Only the latency trace asks, so it resolves its own coordinate systems instead of sharing the
/// frame's: a traced frame pays for the walk, an untraced one never makes it.
pub fn ink(scene: &Scene, batch: &Batch, scissor: Rect<i32, Device>) -> Option<Ink> {
    let (kind, range) = match batch {
        Batch::Quads(range) => (PrimitiveKind::Quad, range),
        Batch::Shaded { range, .. } => (PrimitiveKind::Shaded, range),
        Batch::Shadows(range) => (PrimitiveKind::Shadow, range),
        Batch::Decorations(range) => (PrimitiveKind::Decoration, range),
        Batch::MonoSprites { range, .. } => (PrimitiveKind::MonoSprite, range),
        Batch::SubpixelSprites { range, .. } => (PrimitiveKind::SubpixelSprite, range),
        Batch::ColorSprites { range, .. } => (PrimitiveKind::ColorSprite, range),
        _ => return None,
    };
    let mut spaces = Spaces::new(scene);
    let (cut_left, cut_top) = (scissor.origin.x, scissor.origin.y);
    let cut_right = cut_left + scissor.size.width;
    let cut_bottom = cut_top + scissor.size.height;
    let provenance = scene.provenance(kind);
    let mut pixels = 0u64;
    let mut owners: Vec<(u64, u64)> = Vec::new();
    let mut low = (i32::MAX, i32::MAX);
    let mut high = (i32::MIN, i32::MIN);
    for &slot in scene.remap(kind).get(range.clone())? {
        let (bounds, space) = geometry(scene, kind, slot as usize)?;
        let local = Rect::from_corners(
            Point::new(DevicePx(bounds[0]), DevicePx(bounds[1])),
            Point::new(
                DevicePx(bounds[0] + bounds[2]),
                DevicePx(bounds[1] + bounds[3]),
            ),
        );
        let placed = spaces.on_device(space, local)?;
        let left = (placed.left().0.floor() as i32).max(cut_left);
        let top = (placed.top().0.floor() as i32).max(cut_top);
        let right = (placed.right().0.ceil() as i32).min(cut_right);
        let bottom = (placed.bottom().0.ceil() as i32).min(cut_bottom);
        if right <= left || bottom <= top {
            continue;
        }
        let area = (right - left) as u64 * (bottom - top) as u64;
        pixels += area;
        let revision = provenance
            .get(slot as usize)
            .map_or(0, |chunk| chunk.revision);
        match owners.iter_mut().find(|(owner, _)| *owner == revision) {
            Some((_, owned)) => *owned += area,
            None => owners.push((revision, area)),
        }
        low = (low.0.min(left), low.1.min(top));
        high = (high.0.max(right), high.1.max(bottom));
    }
    let covered = if pixels == 0 {
        Rect::from_corners(Point::new(cut_left, cut_top), Point::new(cut_left, cut_top))
    } else {
        Rect::from_corners(Point::new(low.0, low.1), Point::new(high.0, high.1))
    };
    owners.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Some(Ink {
        pixels,
        covered,
        owners,
    })
}

/// The last quad in `batches` that hides everything drawn before it inside `rect`, as the index
/// of its batch, its position in that batch, and whether it has to be drawn replacing what is there.
///
/// A damage rectangle under an opaque background redraws every layer beneath it: the clear, the
/// window, the page. Only the top opaque layer can show. So the plan starts at that quad and drops
/// the clear and every batch before it. Only a quad outside every group can be such a cover, and
/// only a quad that is solid on every pixel of `rect`: a solid fill, an upright transform, a clip
/// without rounded tests, and bounds that contain `rect` past its corners and its border, with a
/// pixel to spare for the antialiased edge. A quad of a moved chunk is not considered, because the
/// GPU draws it at an offset this does not read.
///
/// A translucent fill hides nothing, but drawn first it lands on the clear, and drawing it with
/// no blend at all writes the same pixels as the clear and the blend did. So a translucent quad
/// is a cover when nothing before it draws in `rect`, and is then drawn replacing.
pub fn cover(
    scene: &Scene,
    batches: &[Reached],
    rect: Rect<i32, Device>,
) -> Option<(usize, usize, bool)> {
    let mut depth = 0usize;
    let mut found = None;
    // Whether anything before the quad being looked at draws in `rect`.
    let mut drawn = false;
    let quads = scene.remap(PrimitiveKind::Quad);
    let provenance = scene.provenance(PrimitiveKind::Quad);
    let inside =
        |reach: &Option<Rect<i32, Device>>| reach.is_none_or(|reach| reach.intersects(rect));
    for (index, Reached { batch, reach }) in batches.iter().enumerate() {
        match batch {
            Batch::Group(group) => {
                if let Some(boundary) = scene.primitives.groups.get(*group) {
                    if boundary.is_start {
                        depth += 1;
                    } else {
                        depth = depth.saturating_sub(1);
                    }
                }
                drawn |= inside(reach);
            }
            Batch::Quads(range) if depth == 0 => {
                if !inside(reach) {
                    continue;
                }
                let whole = reach.is_none_or(|reach| reach.contains_rect(rect));
                let Some(slots) = quads.get(range.clone()) else {
                    drawn = true;
                    continue;
                };
                for (at, &slot) in slots.iter().enumerate() {
                    let moved = provenance
                        .get(slot as usize)
                        .is_some_and(|chunk| scene.chunk_offsets().contains_key(&chunk.revision));
                    if whole && !moved {
                        match covers(scene, slot as usize, rect) {
                            Some(true) => found = Some((index, at, false)),
                            Some(false) if !drawn => found = Some((index, at, true)),
                            _ => {}
                        }
                    }
                    drawn |= moved || reaches(scene, slot as usize, rect);
                }
            }
            _ => drawn |= inside(reach),
        }
    }
    found
}

/// Whether the quad at `slot` may draw in `rect`, read off its bounds where its space is upright.
fn reaches(scene: &Scene, slot: usize, rect: Rect<i32, Device>) -> bool {
    let Some(quad) = scene.primitives.quads.get(slot) else {
        return true;
    };
    let local = Rect::from_corners(
        Point::new(
            DevicePx(quad.bounds[0] - 1.0),
            DevicePx(quad.bounds[1] - 1.0),
        ),
        Point::new(
            DevicePx(quad.bounds[0] + quad.bounds[2] + 1.0),
            DevicePx(quad.bounds[1] + quad.bounds[3] + 1.0),
        ),
    );
    let placed = if quad.transform == SpatialId::VIEWPORT.index() {
        local
    } else {
        let Some(matrix) = scene.spatial.resolve_at(quad.transform) else {
            return true;
        };
        if !maps_affinely(&matrix) {
            return true;
        }
        zgui_geom::transformed_bounds(&matrix, local)
    };
    let rect = Rect::from_corners(
        Point::new(DevicePx(rect.left() as f32), DevicePx(rect.top() as f32)),
        Point::new(
            DevicePx(rect.right() as f32),
            DevicePx(rect.bottom() as f32),
        ),
    );
    placed.intersection(rect).is_some_and(|met| !met.is_empty())
}

/// Whether the quad at `slot` paints its one fill colour on every pixel of `rect`, and if so
/// whether that colour is opaque.
fn covers(scene: &Scene, slot: usize, rect: Rect<i32, Device>) -> Option<bool> {
    let quad = scene.primitives.quads.get(slot)?;
    if quad.fill.kind != PaintKind::Solid as u32 {
        return None;
    }
    let alpha = match quad.fill.id().and_then(|id| scene.paints.get(id)) {
        Some(Paint::Solid(color)) => color.alpha(),
        _ => return None,
    };
    let [tl_x, tl_y, tr_x, tr_y, br_x, br_y, bl_x, bl_y] = quad.radii;
    let [top, right, bottom, left] = quad.border;
    let [x, y, width, height] = quad.bounds;
    let local = |x0: f32, y0: f32, x1: f32, y1: f32| {
        Rect::from_corners(
            Point::new(DevicePx(x0), DevicePx(y0)),
            Point::new(DevicePx(x1), DevicePx(y1)),
        )
    };
    // Where the shader paints the fill and nothing else: inside the border, and out of the four
    // corners, whose curve can reach in as far as their radii.
    let inside = local(x + left, y + top, x + width - right, y + height - bottom);
    let corners = [
        local(x, y, x + tl_x, y + tl_y),
        local(x + width - tr_x, y, x + width, y + tr_y),
        local(x + width - br_x, y + height - br_y, x + width, y + height),
        local(x, y + height - bl_y, x + bl_x, y + height),
    ];
    let matrix = if quad.transform == SpatialId::VIEWPORT.index() {
        None
    } else {
        let matrix = scene.spatial.resolve_at(quad.transform)?;
        let m = &matrix.columns;
        let upright = m[0][1] == 0.0 && m[1][0] == 0.0 && m[0][0] > 0.0 && m[1][1] > 0.0;
        if !upright || !maps_affinely(&matrix) {
            return None;
        }
        Some(matrix)
    };
    let placed = |rect| match &matrix {
        Some(matrix) => zgui_geom::transformed_bounds(matrix, rect),
        None => rect,
    };
    // A pixel inside the edge is fully covered, and the edge's antialiasing band is half a pixel.
    let needed = Rect::from_corners(
        Point::new(
            DevicePx(rect.left() as f32 - 1.0),
            DevicePx(rect.top() as f32 - 1.0),
        ),
        Point::new(
            DevicePx(rect.right() as f32 + 1.0),
            DevicePx(rect.bottom() as f32 + 1.0),
        ),
    );
    if !placed(inside).contains_rect(needed)
        || corners.into_iter().any(|corner| {
            !corner.is_empty()
                && placed(corner)
                    .intersection(needed)
                    .is_some_and(|met| !met.is_empty())
        })
    {
        return None;
    }
    let clip = scene
        .clips
        .resolve_placed(ClipId(quad.clip), &|space| scene.spatial.resolve(space));
    let aabb = clip.aabb;
    let clear = clip.rounded_count == 0
        && clip.dropped == 0
        && clip.mask.is_none()
        && aabb[0] <= rect.left() as f32
        && aabb[1] <= rect.top() as f32
        && aabb[0] + aabb[2] >= rect.right() as f32
        && aabb[1] + aabb[3] >= rect.bottom() as f32;
    clear.then_some(alpha >= 1.0)
}

/// The bounds field and the coordinate-system slot of the primitive at `slot` of `kind`'s array.
fn geometry(scene: &Scene, kind: PrimitiveKind, slot: usize) -> Option<([f32; 4], u32)> {
    let primitives = &scene.primitives;
    Some(match kind {
        PrimitiveKind::Quad => primitives
            .quads
            .get(slot)
            .map(|p| (p.bounds, p.transform))?,
        PrimitiveKind::Shaded => primitives
            .shaded
            .get(slot)
            .map(|p| (p.bounds, p.transform))?,
        PrimitiveKind::Shadow => primitives
            .shadows
            .get(slot)
            .map(|p| (p.bounds, p.transform))?,
        PrimitiveKind::Decoration => primitives
            .decorations
            .get(slot)
            .map(|p| (p.bounds, p.transform))?,
        PrimitiveKind::MonoSprite => primitives
            .mono_sprites
            .get(slot)
            .map(|p| (p.bounds, p.transform))?,
        PrimitiveKind::SubpixelSprite => primitives
            .subpixel_sprites
            .get(slot)
            .map(|p| (p.bounds, p.transform))?,
        PrimitiveKind::ColorSprite => primitives
            .color_sprites
            .get(slot)
            .map(|p| (p.bounds, p.transform))?,
        _ => return None,
    })
}

/// The scene's coordinate systems, resolved once per slot in a row.
///
/// The primitives of one batch mostly share a coordinate system, so the last slot and its matrix
/// are kept.
struct Spaces<'scene> {
    /// The scene whose coordinate systems these are.
    scene: &'scene Scene,
    /// The last slot resolved, and its matrix when it maps onto the device by an affine map.
    last: Option<(u32, Option<Matrix4>)>,
}

impl<'scene> Spaces<'scene> {
    /// No slot resolved yet.
    fn new(scene: &'scene Scene) -> Self {
        Self { scene, last: None }
    }

    /// `rect`, measured in the coordinate system at `slot`, on the device.
    ///
    /// `None` when the system is not bounded on the device: a slot nothing occupies, or a matrix
    /// with a perspective part. Under perspective a corner can be behind the eye, and the bounds of
    /// the corners in front of it are then too small.
    fn on_device(
        &mut self,
        slot: u32,
        rect: Rect<DevicePx, Device>,
    ) -> Option<Rect<DevicePx, Device>> {
        if slot == SpatialId::VIEWPORT.index() {
            return Some(rect);
        }
        let matrix = match self.last {
            Some((held, matrix)) if held == slot => matrix,
            _ => {
                let matrix = self.scene.spatial.resolve_at(slot).filter(maps_affinely);
                self.last = Some((slot, matrix));
                matrix
            }
        };
        Some(zgui_geom::transformed_bounds(&matrix?, rect))
    }
}

/// Whether `matrix` maps the z = 0 plane onto the device with one positive `w` everywhere.
fn maps_affinely(matrix: &Matrix4) -> bool {
    let m = &matrix.columns;
    m[0][3] == 0.0 && m[1][3] == 0.0 && m[3][3] > 0.0
}

#[cfg(test)]
mod tests {
    use super::{Reached, batches};
    use zgui_bits::DamageSet;
    use zgui_geom::{Device, DevicePx, Matrix4, Point, Rect, Size};
    use zgui_scene::{Batch, OwnSpace, Paint, PropertyOwner, Quad, Scene};

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect<DevicePx, Device> {
        Rect::new(
            Point::new(DevicePx(x), DevicePx(y)),
            Size::new(DevicePx(width), DevicePx(height)),
        )
    }

    /// A finished scene of one quad at `at`, drawn under `matrix` when one is given.
    fn one_quad(at: Rect<DevicePx, Device>, matrix: Option<Matrix4>) -> Scene {
        let mut scene = Scene::new();
        scene.begin_frame(Size::new(256, 256));
        let paint = scene.paints.add(Paint::Solid(zgui_color::Color::BLACK));
        let mut quad = Quad::filled(at, paint);
        if let Some(matrix) = matrix {
            let owner = PropertyOwner::new(7).expect("a handle is never the empty word");
            let viewport = scene.spatial.viewport();
            let space =
                scene
                    .spatial
                    .space_of(viewport, owner, OwnSpace::of(Some(matrix), None, false));
            quad = quad.transformed(space);
        }
        scene.push_quad(quad);
        scene.finish(&DamageSet::full());
        scene
    }

    #[test]
    fn a_quad_reaches_its_bounds_and_the_pixels_its_edge_smooths_into() {
        let scene = one_quad(rect(10.5, 20.0, 30.0, 8.0), None);
        let reached = batches(&scene, true);
        assert_eq!(
            reached,
            vec![Reached {
                batch: Batch::Quads(0..1),
                reach: Some(Rect::from_corners(Point::new(8, 18), Point::new(43, 30))),
            }]
        );
    }

    #[test]
    fn an_unbounded_list_reaches_nowhere_in_particular() {
        let scene = one_quad(rect(10.0, 20.0, 30.0, 8.0), None);
        assert_eq!(batches(&scene, false)[0].reach, None);
    }

    #[test]
    fn a_moved_coordinate_system_moves_the_reach() {
        let scene = one_quad(
            rect(0.0, 0.0, 10.0, 10.0),
            Some(Matrix4::translation(100.0, 50.0, 0.0)),
        );
        assert_eq!(
            batches(&scene, true)[0].reach,
            Some(Rect::from_corners(Point::new(98, 48), Point::new(112, 62)))
        );
    }

    #[test]
    fn a_tilt_under_perspective_is_never_culled() {
        // `w` grows across the plane, so the projected corners do not bound what is drawn.
        let tilted = Matrix4::from_columns([
            [1.0, 0.0, 0.0, 0.001],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        let scene = one_quad(rect(0.0, 0.0, 10.0, 10.0), Some(tilted));
        assert_eq!(batches(&scene, true)[0].reach, None);
    }
}
