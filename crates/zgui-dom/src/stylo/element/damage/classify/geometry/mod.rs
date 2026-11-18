//! The properties a change to which costs this pipeline a layout and no more.
//!
//! Every one of them is a property the style engine's own relayout predicate names, so nothing here
//! is a guess about what the engine considers layout-affecting: the set is the engine's, and what
//! is done with it is ours. The properties the engine names that this pipeline does *not* lay
//! anything out for are the ones that are missing, and they are accounted for in
//! [`surface`](super::surface) rather than dropped.
//!
//! What separates these from [`structure`](super::structure) is that the boxes stay: a box holds a
//! clone of its element's style, that clone is refreshed on the frame the cascade moved it, and
//! every number here is read out of the clone each time the box is measured. So the answer is to
//! throw the cached measurement away, not the box.

mod boxes;
mod text;

use style::properties::ComputedValues;

/// Whether nothing that this pipeline measures a box with has changed.
pub(super) fn unchanged(old: &ComputedValues, new: &ComputedValues) -> bool {
    boxes::unchanged(old, new) && text::unchanged(old, new) && spaces(old, new)
}

/// Whether the element has a transform, and a filter, exactly when it had one.
///
/// It is *whether there is one* that matters here rather than what it is (a rotation changing by
/// two degrees moves nothing): a transform starting or stopping gives the box's children another
/// coordinate system and the box another stacking context, both of which its fragments are
/// measured with. The box tree does not change, so this is a layout of the boxes there are. It
/// used to rebuild them: a sheet's view sliding in under a transform built its whole table again
/// when the slide started and when it ended.
fn spaces(old: &ComputedValues, new: &ComputedValues) -> bool {
    old.get_box().has_transform_or_perspective() == new.get_box().has_transform_or_perspective()
        && old.get_effects().filter.0.is_empty() == new.get_effects().filter.0.is_empty()
}
