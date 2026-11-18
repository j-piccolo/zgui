//! The pieces a translation unit is concatenated from.
//!
//! Each is a file beside this one, held as text rather than read from disk, so a consumer of this
//! crate gets the sources by depending on it rather than by knowing where it was unpacked.

/// Every part, under the name a table names it by.
pub const PART_NAMES: [&str; 28] = [
    "blit",
    "blur",
    "clear",
    "common",
    "composite",
    "decoration",
    "effect",
    "external",
    "marks",
    "marks_box",
    "marks_composite",
    "marks_disc",
    "marks_glyph",
    "marks_polyline",
    "paint",
    "quad",
    "sdf",
    "shaded",
    "shaded_coverage",
    "shaded_filter",
    "shaded_paint",
    "shadow",
    "sprite",
    "sprite_color",
    "sprite_mono",
    "subpixel",
    "text",
    "vector",
];

/// The text of the part called `name`.
///
/// # Panics
///
/// Panics when nothing is called `name`. Every caller names a part out of a table in this crate,
/// so an unknown name is a typo in one of those tables rather than anything a consumer can cause.
pub fn part(name: &str) -> &'static str {
    if let Some(text) = from_disk(name) {
        return text;
    }
    built_in(name)
}

/// `ZGUI_WGSL_DIR=<dir>`: a part read from `<dir>/<name>.wgsl` where that file exists, so a shader
/// change is tried without rebuilding (with the pipeline cache pointed somewhere empty, or the
/// cached pipeline is used). Each part is read once and kept for the process.
fn from_disk(name: &str) -> Option<&'static str> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static READ: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let dir = std::env::var_os("ZGUI_WGSL_DIR")?;
    let mut read = READ.get_or_init(Default::default).lock().ok()?;
    if let Some(text) = read.get(name) {
        return Some(text);
    }
    let text =
        std::fs::read_to_string(std::path::Path::new(&dir).join(format!("{name}.wgsl"))).ok()?;
    let text: &'static str = Box::leak(text.into_boxed_str());
    read.insert(name.to_owned(), text);
    Some(text)
}

fn built_in(name: &str) -> &'static str {
    match name {
        "blit" => include_str!("wgsl/blit.wgsl"),
        "blur" => include_str!("wgsl/blur.wgsl"),
        "clear" => include_str!("wgsl/clear.wgsl"),
        "common" => include_str!("wgsl/common.wgsl"),
        "composite" => include_str!("wgsl/composite.wgsl"),
        "decoration" => include_str!("wgsl/decoration.wgsl"),
        "effect" => include_str!("wgsl/effect.wgsl"),
        "external" => include_str!("wgsl/external.wgsl"),
        "marks" => include_str!("wgsl/marks.wgsl"),
        "marks_box" => include_str!("wgsl/marks_box.wgsl"),
        "marks_composite" => include_str!("wgsl/marks_composite.wgsl"),
        "marks_disc" => include_str!("wgsl/marks_disc.wgsl"),
        "marks_glyph" => include_str!("wgsl/marks_glyph.wgsl"),
        "marks_polyline" => include_str!("wgsl/marks_polyline.wgsl"),
        "paint" => include_str!("wgsl/paint.wgsl"),
        "quad" => include_str!("wgsl/quad.wgsl"),
        "sdf" => include_str!("wgsl/sdf.wgsl"),
        "shaded" => include_str!("wgsl/shaded.wgsl"),
        "shaded_coverage" => include_str!("wgsl/shaded_coverage.wgsl"),
        "shaded_filter" => include_str!("wgsl/shaded_filter.wgsl"),
        "shaded_paint" => include_str!("wgsl/shaded_paint.wgsl"),
        "shadow" => include_str!("wgsl/shadow.wgsl"),
        "sprite" => include_str!("wgsl/sprite.wgsl"),
        "sprite_color" => include_str!("wgsl/sprite_color.wgsl"),
        "sprite_mono" => include_str!("wgsl/sprite_mono.wgsl"),
        "subpixel" => include_str!("wgsl/subpixel.wgsl"),
        "text" => include_str!("wgsl/text.wgsl"),
        "vector" => include_str!("wgsl/vector.wgsl"),
        other => panic!("no shading part is called `{other}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::{PART_NAMES, part};

    #[test]
    fn every_named_part_has_text() {
        for name in PART_NAMES {
            assert!(!part(name).trim().is_empty(), "{name} is empty");
        }
    }

    #[test]
    #[should_panic(expected = "no shading part is called")]
    fn a_part_that_does_not_exist_is_a_failure_rather_than_an_empty_string() {
        part("not-a-part");
    }
}
