//! The element behind each chunk the paint encoded, for the latency trace.
//!
//! The renderer notes, for each draw, how many pixels each chunk revision covers (`gpu.draw ...
//! owners=`). The notes written here name the element each revision was encoded for, so a profiler
//! can put the GPU time of every draw on the elements that drew it.

use zgui_dom::{Document, NodeKey};
use zgui_paint::ChunkReport;

/// Element ancestors written before the element itself.
const ANCESTORS: usize = 1;

/// Notes `paint.chunk rev=<revision> kind=<kind> el=<element>` for each chunk in `chunks`.
pub(crate) fn note(document: &Document, chunks: &[ChunkReport]) {
    for chunk in chunks {
        zgui_profile::latency::note_with("paint.chunk", || {
            let element = chunk
                .node
                .map_or_else(|| "<anonymous>".to_owned(), |node| path(document, node));
            format!("rev={} kind={} el={element}", chunk.revision, chunk.kind)
        });
    }
}

/// Notes `group.why el=<element> reason=<reason> a=<element> b=<element>` for each group that took
/// a target: which element kept a fade out of the fold, and what overlapped in it.
pub(crate) fn note_groups(document: &Document, whys: &[zgui_paint::GroupWhy]) {
    let name =
        |node: Option<NodeKey>| node.map_or_else(|| "-".to_owned(), |node| path(document, node));
    for why in whys {
        zgui_profile::latency::note_with("group.why", || {
            format!(
                "el={} reason={} a={} b={}",
                name(why.node),
                why.reason,
                name(why.a),
                name(why.b)
            )
        });
    }
}

/// The element `node` as a selector, after its nearest element ancestors: `a.card > img.poster`.
fn path(document: &Document, node: NodeKey) -> String {
    document.store().index_of(node).map_or_else(
        || "<removed>".to_owned(),
        |index| zgui_dom::describe::path(document, index, ANCESTORS),
    )
}
