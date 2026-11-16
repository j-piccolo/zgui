//! The element behind each chunk the paint encoded, for the latency trace.
//!
//! The renderer notes, for each draw, how many pixels each chunk revision covers (`gpu.draw ...
//! owners=`). The notes written here name the element each revision was encoded for, so a profiler
//! can put the GPU time of every draw on the elements that drew it.

use zgui_dom::{Document, NodeIndex, NodeKey};
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

/// The element `node` as a selector, after its nearest element ancestors, for a trace note.
pub(crate) fn name(document: &Document, node: NodeKey) -> String {
    path(document, node)
}

/// The element `node` as a selector, after its nearest element ancestors: `a.card > img.poster`.
fn path(document: &Document, node: NodeKey) -> String {
    let store = document.store();
    let Some(index) = store.index_of(node) else {
        return "<removed>".to_owned();
    };
    let mut parts = vec![selector(document, index)];
    let mut at = document.node(index).record().parent();
    while let Some(parent) = at {
        if parts.len() > ANCESTORS {
            break;
        }
        let record = document.node(parent).record();
        if record.kind().is_element() {
            parts.push(selector(document, parent));
        }
        at = record.parent();
    }
    parts.reverse();
    parts.join(" > ")
}

/// One node as a selector: its name, then its id and classes.
fn selector(document: &Document, index: NodeIndex) -> String {
    let record = document.node(index).record();
    if !record.kind().is_element() {
        return "#text".to_owned();
    }
    let mut selector = record.local_name().as_str().to_owned();
    if let Some(id) = record.id_attr() {
        selector.push('#');
        selector.push_str(id.as_ref());
    }
    for class in document.store().classes_of(index) {
        selector.push('.');
        selector.push_str(class.0.as_ref());
    }
    selector
}
