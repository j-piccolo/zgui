//! Naming a node the way a stylesheet would, for traces and diagnostics.

use crate::{Document, NodeIndex};

/// The element at `index` as a selector, after up to `ancestors` of its nearest element ancestors:
/// `a.card > img.poster`.
pub fn path(document: &Document, index: NodeIndex, ancestors: usize) -> String {
    let mut parts = vec![selector(document, index)];
    let mut at = document.node(index).record().parent();
    while let Some(parent) = at {
        if parts.len() > ancestors {
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
