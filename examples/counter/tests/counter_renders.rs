//! End-to-end "user app" smoke test: install a recording renderer,
//! mount the counter, drive it through a few state changes, and
//! verify the op stream looks sensible.
//!
//! This is the closest thing to "the example actually runs" until
//! Step 5b of #11 wires the production bootstrap onto the new
//! `render!` / `ElementHandle` surface. Until then this test is the
//! end-to-end validation that the new reactive layer composes
//! correctly for a user-style codebase.

use std::cell::RefCell;
use std::rc::Rc;

use counter::{counter, AppState, CounterProps};
use whisker::flush;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, create_owner, with_owner};
use whisker::runtime::view::{install_renderer, uninstall_renderer, DynRenderer, ElementHandle};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create { id: u32, tag: ElementTag },
    SetAttr { id: u32, key: String, value: String },
    SetStyles { id: u32, css: String },
    Append { parent: u32, child: u32 },
    Event { id: u32, name: String },
}

#[derive(Default)]
struct Recorder {
    next: u32,
    log: Rc<RefCell<Vec<Op>>>,
}

impl Recorder {
    fn new() -> (Self, Rc<RefCell<Vec<Op>>>) {
        let r = Self::default();
        let log = r.log.clone();
        (r, log)
    }
}

impl DynRenderer for Recorder {
    fn create_element(&mut self, tag: ElementTag) -> ElementHandle {
        let id = self.next;
        self.next += 1;
        self.log.borrow_mut().push(Op::Create { id, tag });
        ElementHandle::from_raw(id)
    }
    fn release_element(&mut self, _h: ElementHandle) {}
    fn set_attribute(&mut self, h: ElementHandle, k: &str, v: &str) {
        self.log.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: k.into(),
            value: v.into(),
        });
    }
    fn set_inline_styles(&mut self, h: ElementHandle, css: &str) {
        self.log.borrow_mut().push(Op::SetStyles {
            id: h.id(),
            css: css.into(),
        });
    }
    fn append_child(&mut self, p: ElementHandle, c: ElementHandle) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&mut self, _p: ElementHandle, _c: ElementHandle) {}
    fn set_event_listener(&mut self, h: ElementHandle, name: &str, _cb: Box<dyn Fn() + 'static>) {
        self.log.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
        });
    }
    fn set_root(&mut self, _p: ElementHandle) {}
    fn flush(&mut self) {}
}

fn texts(log: &[Op]) -> Vec<String> {
    log.iter()
        .filter_map(|op| match op {
            Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn counter_initial_render() {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let state = AppState {
        count: RwSignal::new(0),
    };
    let _root = with_owner(owner, || render! { counter(state: state) });

    let ts = texts(&log.borrow());
    // Static label parts + button labels + initial count.
    assert!(ts.contains(&"Count: ".to_string()));
    assert!(ts.contains(&"0".to_string()));
    assert!(ts.contains(&"-1".to_string()));
    assert!(ts.contains(&"reset".to_string()));
    assert!(ts.contains(&"+1".to_string()));
    // "Over 10" message hidden: count = 0, so no "You went over 10!".
    assert!(!ts.contains(&"You went over 10!".to_string()));

    uninstall_renderer(None);
}

#[test]
fn counter_updates_on_signal_write() {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let state = AppState {
        count: RwSignal::new(0),
    };
    let _root = with_owner(owner, || render! { counter(state: state) });

    // Reset log to focus on update behaviour.
    log.borrow_mut().clear();

    state.count.set(11);
    flush();

    let ts = texts(&log.borrow());
    // The dynamic `{count.get()}` element re-rendered with "11".
    assert!(ts.contains(&"11".to_string()));
    // Show flipped true → the "over 10" branch mounted, adding its
    // text. (The element ID is freshly allocated by Show on flip.)
    assert!(ts.contains(&"You went over 10!".to_string()));

    uninstall_renderer(None);
}

#[test]
fn show_swaps_back_when_predicate_flips() {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let state = AppState {
        count: RwSignal::new(15),
    };
    let _root = with_owner(owner, || render! { counter(state: state) });

    // Bring it back below threshold.
    log.borrow_mut().clear();
    state.count.set(3);
    flush();

    let ts = texts(&log.borrow());
    // The dynamic count text becomes "3".
    assert!(ts.contains(&"3".to_string()));
    // The "over 10" branch is unmounted; no new SetAttr emits its
    // text (the prior owner was disposed).
    assert!(!ts.contains(&"You went over 10!".to_string()));

    uninstall_renderer(None);
}
