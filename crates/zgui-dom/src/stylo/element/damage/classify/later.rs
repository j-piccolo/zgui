//! The properties that change only what a later change does.
//!
//! The `transition-*` properties tell the engine whether, and how, a later change of a value
//! animates. Layout, paint and the accessibility projection never read them. A change to them has
//! one effect now: a running transition whose property leaves `transition-property` stops, and its
//! value jumps to the end. That jump is a change of the animated property's own computed value in
//! the same cascade, so the comparison of that property gives the damage for it.
//!
//! The `animation-*` properties are the same kind: they say which keyframes drive which values
//! and how, and nothing lays out or paints from them. Starting, stopping or restarting an
//! animation changes the values it drives (an opacity, a transform), and those are compared on
//! their own. Unnamed here, a class swapped to play a slide again counted as an unexplained
//! difference, which costs the widest answer: the element's boxes and every box below it, built
//! again.

use style::properties::ComputedValues;

use crate::stylo::element::damage::classify::same::same;

/// Whether the transitions and the animations declared on the element agree.
pub(super) fn unchanged(old: &ComputedValues, new: &ComputedValues) -> bool {
    same!(
        old,
        new,
        get_ui,
        [
            transition_behavior,
            transition_delay,
            transition_duration,
            transition_property,
            transition_timing_function,
            animation_composition,
            animation_delay,
            animation_direction,
            animation_duration,
            animation_fill_mode,
            animation_iteration_count,
            animation_name,
            animation_play_state,
            animation_range_end,
            animation_range_start,
            animation_timeline,
            animation_timing_function,
        ]
    )
}
