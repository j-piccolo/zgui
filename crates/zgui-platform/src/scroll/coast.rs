//! Who carries the content on after the fingers lift from a precision surface.

/// Who moves the content after a continuous gesture ends.
///
/// A flick on a precision surface leaves the content moving: it travels on, slows and stops. The
/// motion after the lift has an owner, and the owner differs by desktop.
///
/// A backend that gets this wrong in one direction moves the content twice, once by its own throw
/// and once by the platform's stream. In the other direction the content stops under the fingers
/// the moment they lift.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Coast {
    /// The gesture ends at the lift and nothing follows it.
    ///
    /// What Wayland delivers: the compositor reports the stop and sends no further scroll. The
    /// framework throws the content at the speed the gesture had.
    #[default]
    Framework,
    /// The platform sends the motion after the lift as a stream of momentum scrolls.
    ///
    /// What macOS does. The framework follows the stream and adds no motion of its own.
    Platform,
}

impl Coast {
    /// Whether the framework throws the content when the fingers lift.
    pub const fn framework_throws(self) -> bool {
        matches!(self, Self::Framework)
    }
}

#[cfg(test)]
mod tests {
    use super::Coast;

    #[test]
    fn a_lift_is_the_frameworks_to_carry_unless_the_platform_streams_it() {
        assert_eq!(Coast::default(), Coast::Framework);
        assert!(Coast::Framework.framework_throws());
        assert!(!Coast::Platform.framework_throws());
    }
}
