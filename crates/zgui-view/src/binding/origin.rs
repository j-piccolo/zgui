//! Which closure in which view a binding's writes come from, for the trace.
//!
//! A dynamic value is a closure behind `Rc<dyn Fn>`: by the time a binding writes it, its type
//! (`myshell::gui::sheets::wifi::list_view::{{closure}}`) is gone. While per-element marks are on
//! (`ZGUI_LATENCY_ELEMENTS`), each dynamic value made from a closure files that type's name here,
//! keyed by the closure's allocation; the binding that takes the value takes the name, and makes it
//! current while it writes, so the document's note of the write can say whose it was. Off, nothing
//! is filed and a write reads one empty cell.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    /// Names filed for values no binding has taken yet, by the address of their closure.
    static FILED: RefCell<HashMap<usize, &'static str>> = RefCell::new(HashMap::new());
    /// The name of the closure whose binding is writing now.
    static CURRENT: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Files `name` for the closure at `address`, while per-element marks are on.
pub(crate) fn file(address: usize, name: &'static str) {
    if zgui_profile::latency::tracing_elements() {
        FILED.with(|filed| filed.borrow_mut().insert(address, name));
    }
}

/// The name filed for the closure at `address`, taken out.
pub(crate) fn take(address: usize) -> Option<&'static str> {
    if !zgui_profile::latency::tracing_elements() {
        return None;
    }
    FILED.with(|filed| filed.borrow_mut().remove(&address))
}

/// Runs `write` with `name` as the writing closure.
pub(crate) fn writing<R>(name: Option<&'static str>, write: impl FnOnce() -> R) -> R {
    let Some(name) = name else {
        return write();
    };
    let outer = CURRENT.with(|current| current.replace(Some(name)));
    let result = write();
    CURRENT.with(|current| current.set(outer));
    result
}

/// The closure whose binding is writing now, by its type's name, if a trace asked for it.
pub fn current() -> Option<&'static str> {
    CURRENT.with(Cell::get)
}
