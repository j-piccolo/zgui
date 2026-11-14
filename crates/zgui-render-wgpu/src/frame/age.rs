//! What each presented image is missing, so that the copy to the surface writes only that.
//!
//! Each frame copies the composed target onto the image it presents. A full-screen copy costs
//! about a millisecond on an integrated GPU. A swap chain image keeps the frame last copied into
//! it, so the copy needs only the union of the changes to the composed target since that frame.
//!
//! An unknown image gets a full copy: the first use, after a reconfiguration, or an image older
//! than the kept history.

use std::collections::VecDeque;

use zgui_geom::{Device, Rect};

/// How many frames of changes are kept: enough for any swap chain this renderer configures.
const KEPT: usize = 8;

/// Past this share of the surface, one whole copy is cheaper than many scissored ones.
const WHOLE_SHARE: f64 = 0.6;

/// More rectangles than this are copied whole rather than one by one.
const MOST_RECTS: usize = 32;

/// How a presented image is told apart from the others.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Image {
    /// A swap chain image, or a texture that stands in for a surface. Identified by the texture,
    /// because each present makes a new view.
    Texture(wgpu::Texture),
}

/// The changes to the composed target, frame by frame, and which frame each image was last
/// brought up to.
#[derive(Debug, Default)]
pub struct PresentHistory {
    /// The number of the last frame that changed the composed target.
    serial: u64,
    /// What each recent frame changed, oldest first.
    changes: VecDeque<(u64, Vec<Rect<i32, Device>>)>,
    /// The frame each recently presented image holds.
    images: Vec<(Image, u64)>,
}

impl PresentHistory {
    /// Records one frame's changes to the composed target.
    pub fn changed(&mut self, rects: Vec<Rect<i32, Device>>) {
        self.serial += 1;
        self.changes.push_back((self.serial, rects));
        while self.changes.len() > KEPT {
            self.changes.pop_front();
        }
    }

    /// Forgets every image: what they hold can no longer be trusted, or they are gone.
    pub fn forget(&mut self) {
        self.images.clear();
    }

    /// What `image` needs copied to hold the latest frame, within `surface`: `None` for the whole
    /// surface, and an empty list when it is already up to date.
    pub fn missing(
        &self,
        image: &Image,
        surface: Rect<i32, Device>,
    ) -> Option<Vec<Rect<i32, Device>>> {
        let held = self.images.iter().find(|(known, _)| known == image)?.1;
        if held == self.serial {
            return Some(Vec::new());
        }
        // The changes since `held` must all still be in the history.
        let oldest = self.changes.front()?.0;
        if held + 1 < oldest {
            return None;
        }
        let rects: Vec<_> = self
            .changes
            .iter()
            .filter(|(serial, _)| *serial > held)
            .flat_map(|(_, rects)| rects.iter().filter_map(|rect| rect.intersection(surface)))
            .collect();
        let area: i64 = rects
            .iter()
            .map(|rect| i64::from(rect.size.width) * i64::from(rect.size.height))
            .sum();
        let whole = i64::from(surface.size.width) * i64::from(surface.size.height);
        #[expect(clippy::cast_precision_loss, reason = "a ratio of two areas")]
        let share = area as f64 / whole.max(1) as f64;
        if rects.len() > MOST_RECTS || share > WHOLE_SHARE {
            return None;
        }
        Some(rects)
    }

    /// Records that `image` now holds the latest frame.
    pub fn presented(&mut self, image: Image) {
        let serial = self.serial;
        if let Some(entry) = self.images.iter_mut().find(|(known, _)| *known == image) {
            entry.1 = serial;
            return;
        }
        self.images.push((image, serial));
        if self.images.len() > KEPT {
            self.images.remove(0);
        }
    }
}
