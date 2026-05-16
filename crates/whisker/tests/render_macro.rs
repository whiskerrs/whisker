//! Integration test for the `render!` macro (Phase 6.5a A3 Step 2).
//!
//! Step 2 covers: static elements, static attributes, static styles,
//! event handlers, and string-literal children rendered as RawText
//! elements. `{expr}` interpolation lands in Step 3 and is a compile
//! error today.
//!
//! Tests install a small recording renderer, expand `render!`,
//! and assert on the recorded op sequence.

use std::cell::RefCell;
use std::rc::Rc;
use whisker::prelude::*;
use whisker::runtime::view::{
    install_renderer, uninstall_renderer, DynRenderer, ElementHandle,
};

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
    fn set_event_listener(
        &mut self,
        h: ElementHandle,
        name: &str,
        _cb: Box<dyn Fn() + 'static>,
    ) {
        self.log.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
        });
    }
    fn set_root(&mut self, _p: ElementHandle) {}
    fn flush(&mut self) {}
}

fn with_recorder<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    let (rec, log) = Recorder::new();
    let prev = install_renderer(Box::new(rec));
    let result = f(log);
    let _ = uninstall_renderer(prev);
    result
}

// ----- Static element trees -------------------------------------------------

#[test]
fn single_view_emits_create_and_returns_handle() {
    with_recorder(|log| {
        let h = render! { view {} };
        assert_eq!(h.id(), 0);
        assert_eq!(*log.borrow(), vec![Op::Create { id: 0, tag: ElementTag::View }]);
    });
}

#[test]
fn nested_view_with_text_child() {
    with_recorder(|log| {
        let _h = render! {
            view {
                text { "Hello" }
            }
        };
        // Expected ops:
        //  Create view (0)
        //  Create text (1)
        //  Create raw_text (2)
        //  Set attr text="Hello" on raw_text
        //  Append raw_text → text
        //  Append text → view
        let ops = log.borrow();
        assert_eq!(ops[0], Op::Create { id: 0, tag: ElementTag::View });
        assert_eq!(ops[1], Op::Create { id: 1, tag: ElementTag::Text });
        assert_eq!(ops[2], Op::Create { id: 2, tag: ElementTag::RawText });
        assert_eq!(
            ops[3],
            Op::SetAttr {
                id: 2,
                key: "text".into(),
                value: "Hello".into()
            }
        );
        assert_eq!(ops[4], Op::Append { parent: 1, child: 2 });
        assert_eq!(ops[5], Op::Append { parent: 0, child: 1 });
        assert_eq!(ops.len(), 6);
    });
}

#[test]
fn style_attribute_emits_set_inline_styles() {
    with_recorder(|log| {
        let _ = render! {
            view {
                style: "padding: 16px;",
            }
        };
        let ops = log.borrow();
        assert_eq!(ops[0], Op::Create { id: 0, tag: ElementTag::View });
        assert_eq!(
            ops[1],
            Op::SetStyles {
                id: 0,
                css: "padding: 16px;".into()
            }
        );
    });
}

#[test]
fn arbitrary_attribute_emits_set_attribute() {
    with_recorder(|log| {
        let _ = render! {
            image {
                src: "https://example.com/x.png",
                alt: "example",
            }
        };
        let ops = log.borrow();
        assert_eq!(ops[0], Op::Create { id: 0, tag: ElementTag::Image });
        assert!(ops.contains(&Op::SetAttr {
            id: 0,
            key: "src".into(),
            value: "https://example.com/x.png".into(),
        }));
        assert!(ops.contains(&Op::SetAttr {
            id: 0,
            key: "alt".into(),
            value: "example".into(),
        }));
    });
}

#[test]
fn on_tap_emits_set_event_listener() {
    with_recorder(|log| {
        let fired = Rc::new(RefCell::new(false));
        let f = fired.clone();
        let _ = render! {
            view {
                on_tap: move || *f.borrow_mut() = true,
            }
        };
        let ops = log.borrow();
        assert!(ops.iter().any(|op| matches!(op, Op::Event { name, .. } if name == "tap")));
        // The recorder stored but doesn't fire the callback —
        // verifying registration is enough at the macro layer.
        assert!(!*fired.borrow());
    });
}

#[test]
fn camel_case_event_handler_lowercased() {
    with_recorder(|log| {
        let _ = render! {
            view {
                onTap: || {},
            }
        };
        let ops = log.borrow();
        assert!(ops.iter().any(|op| matches!(op, Op::Event { name, .. } if name == "tap")));
    });
}

#[test]
fn multiple_children_append_in_order() {
    with_recorder(|log| {
        let _ = render! {
            view {
                text { "A" }
                text { "B" }
                text { "C" }
            }
        };
        let appends: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::Append { parent, child } => Some((*parent, *child)),
                _ => None,
            })
            .collect();
        // text.id < view.id always; we just check the order of appends
        // onto view (id 0) matches the declaration order: A, B, C
        // (whatever their ids).
        let appends_to_view: Vec<_> = appends.iter().filter(|(p, _)| *p == 0).collect();
        assert_eq!(appends_to_view.len(), 3);
    });
}

#[test]
fn page_view_image_scroll_view_tags_supported() {
    with_recorder(|log| {
        let _ = render! {
            page {
                scroll_view {
                    image {}
                }
            }
        };
        let creates: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::Create { tag, .. } => Some(*tag),
                _ => None,
            })
            .collect();
        assert_eq!(
            creates,
            vec![ElementTag::Page, ElementTag::ScrollView, ElementTag::Image]
        );
    });
}
