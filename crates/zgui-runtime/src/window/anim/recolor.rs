//! Moving an animated colour by rewriting the paint slots a box owns.
//!
//! A box whose animated colours are drawn through owned slots records which slots in its paint
//! signature, and none of the values. So a frame that moves only the colours replays the box's
//! chunk, and the work is here: the slots rewritten in place, and the box's ink put into the
//! damage. Nothing sends the fragment pass to the box, because a colour moves no geometry and no
//! hit entry.
//!
//! The ink is damaged for every override that [`OwnedPaints`] accepts, also when the slots could
//! not take the colour. The fragment pass damages nothing for a box whose shared style paints
//! nothing, and a transition to transparent is exactly such a box. An override it refuses, an
//! opacity for example, gets no damage here: the fragment pass damages it.

use zgui_dom::NodeKey;
use zgui_dom::side::{AnimOverride, BoxKey};
use zgui_geom::{DevicePx, Edges};
use zgui_layout::FragmentKind;
use zgui_layout::fragment::diff::pixels;
use zgui_paint::OwnedPaints;
use zgui_scene::{PaintRewrite, PropertyOwner};

use crate::window::anim::place::Writing;

impl Writing<'_> {
    /// Writes `values` into the slots `node`'s boxes own, and reports whether that is all the
    /// frame owes them.
    ///
    /// True only while every fragment of the boxes has a record encoded through exactly the slots
    /// `values` drives, and each of those slots took its colour. A box with no record, or a nested
    /// inline box that the lines it is on paint, takes the fragment pass.
    pub(super) fn write_colors(&mut self, node: NodeKey, values: &AnimOverride) -> bool {
        // Text is drawn through no owned slot, so a moving `color` repaints the lines.
        if !OwnedPaints::any(values) || values.color.is_some() {
            if values.color.is_some() {
                self.damage_subtree_ink(node);
            }
            return false;
        }
        self.damage_own_ink(node);
        let boxes = self.layout.boxes_of(node);
        if boxes.is_empty() {
            return false;
        }
        let cache = self.painter.cache();
        let recorded_owned = boxes.iter().all(|key| {
            let fragments = self.layout.fragments_of_box(*key);
            let Some(owned) = OwnedPaints::of(values, self.border_of(*key)) else {
                return false;
            };
            let signature = owned.signature();
            self.layout
                .get(*key)
                .is_some_and(|box_| !box_.is_nested_inline())
                && !fragments.is_empty()
                && fragments
                    .iter()
                    .all(|frag| cache.anim_signature(*frag) == Some(signature))
        });
        if !recorded_owned {
            return false;
        }
        let mut written = true;
        for key in boxes {
            let border = self.border_of(*key);
            let Some(owned) = OwnedPaints::of(values, border) else {
                return false;
            };
            for (name, paint) in owned.paints(PropertyOwner::of(*key), values, border) {
                written &= self.scene.paints.rewrite(name, paint) == PaintRewrite::Written;
            }
        }
        written
    }

    /// Whether `node`'s `color` reaches nothing but text, and the colours of its own boxes the
    /// override carries.
    ///
    /// A box below it follows the colour when it computed the same colour as its parent, which is
    /// what inheriting it leaves; one that computed another sets its own and is passed over with
    /// everything below it. Whatever follows may draw text and nothing else in the colour: no
    /// picture or drawing (an icon is drawn in `currentColor`), no shadow or decoration, and, below
    /// the element itself, no `currentColor` background, border or outline.
    pub(super) fn own_text_only(&self, node: NodeKey) -> bool {
        let boxes = self.layout.boxes_of(node);
        let Some(first) = boxes.first().and_then(|key| self.layout.get(*key)) else {
            return false;
        };
        let color = first.style.get_inherited_text().color;
        let mut stack: Vec<(BoxKey, _)> = boxes.iter().map(|key| (*key, color)).collect();
        while let Some((key, parent)) = stack.pop() {
            let Some(box_) = self.layout.get(key) else {
                return false;
            };
            let style = &box_.style;
            let element = box_.source.is_some_and(|source| source != node);
            let text = style.get_inherited_text();
            // One that would run a transition of its own as the colour moves under it.
            if element && zgui_style::transitions_text_color(style) {
                return false;
            }
            if element && text.color != parent {
                continue;
            }
            if !style.get_effects().box_shadow.0.is_empty()
                || !text.text_shadow.0.is_empty()
                || !style.get_text().text_decoration_line.is_empty()
            {
                return false;
            }
            if element {
                let border = style.get_border();
                // `currentColor` is every border's and outline's initial colour; only one drawn
                // reads it.
                let sides = [
                    (
                        &border.border_top_color,
                        border.border_top_style,
                        &border.border_top_width,
                    ),
                    (
                        &border.border_right_color,
                        border.border_right_style,
                        &border.border_right_width,
                    ),
                    (
                        &border.border_bottom_color,
                        border.border_bottom_style,
                        &border.border_bottom_width,
                    ),
                    (
                        &border.border_left_color,
                        border.border_left_style,
                        &border.border_left_width,
                    ),
                ];
                let outline = style.get_outline();
                if style.get_background().background_color.is_currentcolor()
                    || sides.iter().any(|(color, style, width)| {
                        color.is_currentcolor()
                            && !style.none_or_hidden()
                            && width.0 > zgui_css::Au(0)
                    })
                    || (outline.outline_color.is_currentcolor()
                        && !outline.outline_style.none_or_hidden()
                        && outline.outline_width.0 > zgui_css::Au(0))
                {
                    return false;
                }
            }
            for frag in self.layout.fragments_of_box(key) {
                let Some(fragment) = self.layout.fragment(*frag) else {
                    continue;
                };
                match fragment.kind {
                    FragmentKind::Line { .. }
                    | FragmentKind::TextRun { .. }
                    | FragmentKind::Box => {}
                    _ => return false,
                }
            }
            stack.extend(box_.children.iter().map(|child| (*child, text.color)));
        }
        true
    }

    /// The border widths of the box `key`, read off its first fragment as the emission reads them.
    fn border_of(&self, key: BoxKey) -> Edges<DevicePx> {
        self.layout
            .fragments_of_box(key)
            .first()
            .and_then(|frag| self.layout.fragment(*frag))
            .map_or(Edges::ZERO, |fragment| fragment.border)
    }

    /// Damages `node`'s ink when the override it dropped held colours [`OwnedPaints`] accepts.
    pub(super) fn damage_uncolored(&mut self, node: NodeKey, was: &AnimOverride) {
        if was.color.is_some() {
            self.damage_subtree_ink(node);
        } else if OwnedPaints::any(was) {
            self.damage_own_ink(node);
        }
    }

    /// Damages everything `node`'s boxes and the boxes below them draw, cut to each one's clip:
    /// the lines a moving `color` paints hang from anonymous boxes below the element.
    fn damage_subtree_ink(&mut self, node: NodeKey) {
        let scene = &*self.scene;
        let resolve = |id| scene.spatial.resolve(id);
        for key in self.layout.boxes_of(node) {
            for frag in self.layout.fragments_of_box(*key) {
                let Some(fragment) = self.layout.fragment(*frag) else {
                    continue;
                };
                let ink = fragment.on_device_in(fragment.subtree_ink, &scene.spatial);
                let admitted = scene.clips.bounds_placed(fragment.clip, &resolve);
                if let Some(rect) = ink.intersection(admitted)
                    && !rect.is_empty()
                {
                    self.damage.absorb(pixels(rect));
                }
            }
        }
    }

    /// Damages the ink of every fragment of `node`'s boxes, where it is on the device now.
    ///
    /// Cut to what each fragment's clip chain admits, and rounded outwards to whole pixels, as the
    /// fragment pass does for a repainted box.
    fn damage_own_ink(&mut self, node: NodeKey) {
        let scene = &*self.scene;
        let resolve = |id| scene.spatial.resolve(id);
        for key in self.layout.boxes_of(node) {
            for frag in self.layout.fragments_of_box(*key) {
                let Some(fragment) = self.layout.fragment(*frag) else {
                    continue;
                };
                let ink = fragment.on_device_in(fragment.ink, &scene.spatial);
                let admitted = scene.clips.bounds_placed(fragment.clip, &resolve);
                if let Some(rect) = ink.intersection(admitted)
                    && !rect.is_empty()
                {
                    self.damage.absorb(pixels(rect));
                }
            }
        }
    }
}
