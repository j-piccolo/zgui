//! The chunk report: the element behind every primitive a frame draws out of a chunk.
//!
//! A profiler reads the GPU time of each draw, the chunk revision of each instance it draws, and
//! this report. Together they give GPU time per element, so every instance has to name a reported
//! chunk, and a replayed chunk has to keep the revision its encoding reported.

mod support;

use std::collections::{HashMap, HashSet};

use support::{Element, Harness};
use zgui_dom::NodeKey;
use zgui_paint::PaintReport;
use zgui_scene::PrimitiveKind;

/// Every kind a renderer draws instanced, which is every kind with provenance.
const INSTANCED: [PrimitiveKind; 7] = [
    PrimitiveKind::Quad,
    PrimitiveKind::Shaded,
    PrimitiveKind::Shadow,
    PrimitiveKind::Decoration,
    PrimitiveKind::MonoSprite,
    PrimitiveKind::SubpixelSprite,
    PrimitiveKind::ColorSprite,
];

/// A card with a shadow and a bordered ring in it, and a badge beside it.
fn page() -> Harness {
    Harness::new(
        Element::new("root").children(vec![
            Element::new("card").children(vec![Element::new("ring")]),
            Element::new("badge"),
        ]),
        "root { display: block }
         card { display: block; width: 100px; height: 50px; background: #eee;
                box-shadow: 0 2px 4px #0004 }
         ring { display: block; width: 20px; height: 20px; border: 2px solid red }
         badge { display: block; width: 30px; height: 10px; background: #123 }",
    )
}

/// The node each reported revision was encoded for.
fn owners(report: &PaintReport) -> HashMap<u64, Option<NodeKey>> {
    report
        .chunks
        .iter()
        .map(|chunk| (chunk.revision, chunk.node))
        .collect()
}

/// The node of the first element called `name`.
fn key(harness: &Harness, name: &str) -> NodeKey {
    let index = harness.elements(name)[0];
    harness.document().store().key_of(index)
}

/// The nodes every instanced primitive of the last frame names through `owners`.
fn drawn_for(harness: &Harness, owners: &HashMap<u64, Option<NodeKey>>) -> HashSet<NodeKey> {
    let mut drawn = HashSet::new();
    let mut instances = 0;
    for kind in INSTANCED {
        for slot in harness.scene().provenance(kind) {
            instances += 1;
            assert!(
                !slot.is_transient(),
                "a {kind:?} was drawn outside any chunk, so no element can be named for it"
            );
            let node = owners
                .get(&slot.revision)
                .unwrap_or_else(|| panic!("revision {} was never reported", slot.revision));
            drawn.extend(*node);
        }
    }
    assert!(
        instances > 0,
        "the page drew nothing, so the test checks nothing"
    );
    drawn
}

#[test]
fn every_instance_names_the_element_it_was_encoded_for() {
    let mut harness = page();
    let report = harness.paint_reporting_chunks();
    let drawn = drawn_for(&harness, &owners(&report));
    for name in ["card", "ring", "badge"] {
        assert!(
            drawn.contains(&key(&harness, name)),
            "nothing drawn was attributed to `{name}`"
        );
    }
    assert!(
        !drawn.contains(&key(&harness, "root")),
        "`root` paints nothing, so nothing drawn can be attributed to it"
    );
}

#[test]
fn a_replayed_chunk_keeps_the_revision_its_encoding_reported() {
    let mut harness = page();
    let first = owners(&harness.paint_reporting_chunks());
    let second = harness.paint_reporting_chunks();
    assert!(
        second.chunks.is_empty(),
        "an unchanged page replays every chunk, so the second frame encodes nothing: {:?}",
        second.chunks
    );
    let drawn = drawn_for(&harness, &first);
    assert!(drawn.contains(&key(&harness, "card")));
}

#[test]
fn nothing_is_reported_unless_asked() {
    let mut harness = page();
    assert!(harness.paint_everything().chunks.is_empty());
}
