//! Tests for the view layer.
//!
//! Uses a small inline `RecordingRenderer` that records every
//! dispatched call into a `Vec<Op>`, which test assertions then
//! inspect.

use super::*;
use crate::element::ElementTag;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create { id: u32, tag: ElementTag },
    CreateByName { id: u32, tag_name: String },
    Release { id: u32 },
    SetAttr { id: u32, key: String, value: String },
    SetStyles { id: u32, css: String },
    Append { parent: u32, child: u32 },
    Remove { parent: u32, child: u32 },
    Event { id: u32, name: String },
    SetRoot { id: u32 },
    Flush,
}

#[derive(Default)]
struct RecordingRenderer {
    next_id: u32,
    ops: std::rc::Rc<std::cell::RefCell<Vec<Op>>>,
}

impl RecordingRenderer {
    fn with_log() -> (Self, std::rc::Rc<std::cell::RefCell<Vec<Op>>>) {
        let renderer = Self::default();
        let log = renderer.ops.clone();
        (renderer, log)
    }
}

impl DynRenderer for RecordingRenderer {
    fn create_element(&mut self, tag: ElementTag) -> Element {
        let id = self.next_id;
        self.next_id += 1;
        self.ops.borrow_mut().push(Op::Create { id, tag });
        Element::from_raw(id)
    }
    fn create_element_by_name(&mut self, tag_name: &str) -> Element {
        let id = self.next_id;
        self.next_id += 1;
        self.ops.borrow_mut().push(Op::CreateByName {
            id,
            tag_name: tag_name.into(),
        });
        Element::from_raw(id)
    }
    fn release_element(&mut self, h: Element) {
        self.ops.borrow_mut().push(Op::Release { id: h.id() });
    }
    fn set_attribute(&mut self, h: Element, key: &str, value: &str) {
        self.ops.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: key.into(),
            value: value.into(),
        });
    }
    fn set_inline_styles(&mut self, h: Element, css: &str) {
        self.ops.borrow_mut().push(Op::SetStyles {
            id: h.id(),
            css: css.into(),
        });
    }
    fn append_child(&mut self, parent: Element, child: Element) {
        self.ops.borrow_mut().push(Op::Append {
            parent: parent.id(),
            child: child.id(),
        });
    }
    fn remove_child(&mut self, parent: Element, child: Element) {
        self.ops.borrow_mut().push(Op::Remove {
            parent: parent.id(),
            child: child.id(),
        });
    }
    fn set_event_listener(&mut self, h: Element, name: &str, _callback: Box<dyn Fn() + 'static>) {
        self.ops.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
        });
    }
    fn set_root(&mut self, page: Element) {
        self.ops.borrow_mut().push(Op::SetRoot { id: page.id() });
    }
    fn flush(&mut self) {
        self.ops.borrow_mut().push(Op::Flush);
    }
}

// ----- Renderer installation -----------------------------------------------

#[test]
fn dispatch_routes_to_installed_renderer() {
    let (renderer, log) = RecordingRenderer::with_log();
    let h = with_installed_renderer(Box::new(renderer), || {
        let h = create_element(ElementTag::View);
        set_attribute(h, "k", "v");
        flush();
        h
    });
    assert_eq!(h.id(), 0);
    let ops = log.borrow();
    assert_eq!(
        ops.as_slice(),
        &[
            Op::Create {
                id: 0,
                tag: ElementTag::View
            },
            Op::SetAttr {
                id: 0,
                key: "k".into(),
                value: "v".into()
            },
            Op::Flush,
        ],
    );
}

#[test]
fn dispatch_no_renderer_is_silent_noop() {
    // No installed renderer.
    let h = create_element(ElementTag::View);
    // Returns the sentinel `u32::MAX` and prints to stderr in debug.
    assert_eq!(h.id(), u32::MAX);
}

#[test]
fn install_returns_previous_renderer() {
    let (r1, _) = RecordingRenderer::with_log();
    let (r2, _) = RecordingRenderer::with_log();
    let prev = install_renderer(Box::new(r1));
    assert!(prev.is_none());
    let prev = install_renderer(Box::new(r2));
    assert!(prev.is_some());
    uninstall_renderer(prev);
    // Restore to first installed.
    let still_installed = current_renderer_id();
    assert!(still_installed.is_some());
    uninstall_renderer(None);
    assert!(current_renderer_id().is_none());
}

// ----- IntoView impls -------------------------------------------------------

#[test]
fn element_handle_into_view() {
    let h = Element::from_raw(7);
    let v = h.into_view();
    match v {
        View::Element(e) => assert_eq!(e.id(), 7),
        _ => panic!("expected Element"),
    }
}

#[test]
fn unit_into_empty() {
    let v: View = ().into_view();
    assert!(matches!(v, View::Empty));
    assert!(v.elements().is_empty());
}

#[test]
fn option_some_and_none() {
    let some_h = Some(Element::from_raw(3));
    let none_h: Option<Element> = None;
    assert_eq!(some_h.into_view().elements(), vec![Element::from_raw(3)]);
    assert_eq!(none_h.into_view().elements(), Vec::<Element>::new());
}

#[test]
fn tuple_into_fragment_preserves_order() {
    let v = (
        Element::from_raw(10),
        Element::from_raw(20),
        Element::from_raw(30),
    )
        .into_view();
    assert_eq!(
        v.elements(),
        vec![
            Element::from_raw(10),
            Element::from_raw(20),
            Element::from_raw(30),
        ]
    );
}

#[test]
fn nested_tuples_flatten_in_order() {
    let v = (
        Element::from_raw(1),
        (Element::from_raw(2), Element::from_raw(3)),
        Element::from_raw(4),
    )
        .into_view();
    assert_eq!(
        v.elements(),
        vec![
            Element::from_raw(1),
            Element::from_raw(2),
            Element::from_raw(3),
            Element::from_raw(4),
        ]
    );
}

#[test]
fn view_attach_appends_each_leaf() {
    let (renderer, log) = RecordingRenderer::with_log();
    with_installed_renderer(Box::new(renderer), || {
        let parent = create_element(ElementTag::View);
        let frag = View::Fragment(vec![
            View::Element(Element::from_raw(100)),
            View::Element(Element::from_raw(200)),
            View::Empty,
            View::Element(Element::from_raw(300)),
        ]);
        frag.attach_to(parent);
    });
    let ops = log.borrow();
    let appends: Vec<_> = ops
        .iter()
        .filter(|o| matches!(o, Op::Append { .. }))
        .collect();
    assert_eq!(appends.len(), 3, "Empty fragments must be skipped");
}
