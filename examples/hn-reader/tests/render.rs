//! Smoke test for the hn-reader example: mounts the component and
//! checks that the initial render shows the loading banner.
//!
//! We don't try to exercise the full fetch path here — that would
//! require either a real HTTP round-trip (flaky / offline-fails)
//! or a mock HTTP layer (heavy for an example). The fetch worker
//! does run, but its `run_on_main_thread` call no-ops in tests
//! (no dispatcher is registered without bootstrap), so the state
//! stays at `Loading` and the loading text is what we render.

use std::cell::RefCell;
use std::rc::Rc;

use hn_reader::{HnReader, HnReaderProps};
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, create_owner, with_owner};
use whisker::runtime::view::{install_renderer, uninstall_renderer, DynRenderer, Element};

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
    fn create_element(&mut self, tag: ElementTag) -> Element {
        let id = self.next;
        self.next += 1;
        self.log.borrow_mut().push(Op::Create { id, tag });
        Element::from_raw(id)
    }
    fn create_element_by_name(&mut self, _tag_name: &str) -> Element {
        let id = self.next;
        self.next += 1;
        self.log.borrow_mut().push(Op::Create {
            id,
            tag: ElementTag::View,
        });
        Element::from_raw(id)
    }
    fn release_element(&mut self, _h: Element) {}
    fn set_attribute(&mut self, h: Element, k: &str, v: &str) {
        self.log.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: k.into(),
            value: v.into(),
        });
    }
    fn set_inline_styles(&mut self, h: Element, css: &str) {
        self.log.borrow_mut().push(Op::SetStyles {
            id: h.id(),
            css: css.into(),
        });
    }
    fn append_child(&mut self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&mut self, _p: Element, _c: Element) {}
    fn set_event_listener(&mut self, h: Element, name: &str, _cb: Box<dyn Fn() + 'static>) {
        self.log.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
        });
    }
    fn set_root(&mut self, _p: Element) {}
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
fn initial_render_shows_loading_banner() {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let _root = with_owner(owner, || render! { HnReader() });

    let ts = texts(&log.borrow());
    assert!(
        ts.iter().any(|t| t.contains("Loading top stories")),
        "expected loading banner in initial render, got texts: {ts:?}",
    );
    // The "Hacker News" header is static and should always render.
    assert!(
        ts.contains(&"Hacker News".to_string()),
        "expected header text 'Hacker News', got: {ts:?}",
    );

    uninstall_renderer(None);
}
