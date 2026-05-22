//! `#[whisker::native_element]` end-to-end tests.
//!
//! Verifies the proc-macro lowers a tag-name + prop list into:
//! - `XxxProps::builder().<prop>(v).build()` shape
//! - a body that calls `view::create_element_by_name(tag)`
//! - per-prop `apply_styles` / `apply_attr` (Static set-once,
//!   Dynamic effect-wrapped) routing
//!
//! The in-memory `Recorder` captures every dispatched op into
//! `Op::*` so assertions can verify the underlying tag-name + per-
//! attribute set sequence.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, create_owner, dispose_owner};
use whisker::runtime::view::{install_renderer, uninstall_renderer, DynRenderer, Element};
use whisker::{flush, with_owner};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    CreateByName { id: u32, tag_name: String },
    SetAttr { id: u32, key: String, value: String },
    SetStyles { id: u32, css: String },
}

#[derive(Default)]
struct Recorder {
    next: u32,
    log: Rc<RefCell<Vec<Op>>>,
}

impl Recorder {
    fn with_log() -> (Self, Rc<RefCell<Vec<Op>>>) {
        let r = Self::default();
        let log = r.log.clone();
        (r, log)
    }
}

impl DynRenderer for Recorder {
    fn create_element(&mut self, _tag: ElementTag) -> Element {
        let id = self.next;
        self.next += 1;
        Element::from_raw(id)
    }
    fn create_element_by_name(&mut self, tag_name: &str) -> Element {
        let id = self.next;
        self.next += 1;
        self.log.borrow_mut().push(Op::CreateByName {
            id,
            tag_name: tag_name.into(),
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
    fn append_child(&mut self, _p: Element, _c: Element) {}
    fn remove_child(&mut self, _p: Element, _c: Element) {}
    fn set_event_listener(&mut self, _h: Element, _name: &str, _cb: Box<dyn Fn() + 'static>) {}
    fn set_root(&mut self, _p: Element) {}
    fn flush(&mut self) {}
}

fn with_recorder_and_owner<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    __reset_for_tests();
    let (rec, log) = Recorder::with_log();
    let prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);
    let out = with_owner(owner, || f(log));
    dispose_owner(owner);
    uninstall_renderer(prev);
    out
}

// ---- Native element declarations ------------------------------------------

#[whisker::native_element("x-zero-props")]
pub fn x_zero_props() {}

#[whisker::native_element("x-styled")]
pub fn x_styled(style: Signal<String>) {}

#[whisker::native_element("x-input")]
pub fn x_input(value: Signal<String>, placeholder: Signal<String>) {}

// ---- Tests -----------------------------------------------------------------

#[test]
fn zero_props_creates_element_with_tag_name() {
    with_recorder_and_owner(|log| {
        let _h = render! {
            XZeroProps()
        };
        let creates: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::CreateByName { tag_name, .. } => Some(tag_name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(creates, vec!["x-zero-props".to_string()]);
    });
}

#[test]
fn style_prop_routes_through_set_inline_styles() {
    // The `style` prop is special — it must call set_inline_styles
    // (Lynx's SetRawInlineStyles), not set_attribute. Mirrors what
    // built-in `view(style: …)` does.
    with_recorder_and_owner(|log| {
        let _h = render! {
            XStyled(style: "background: red; height: 8px;")
        };
        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetStyles { css, .. } => Some(css.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(styles, vec!["background: red; height: 8px;".to_string()]);
    });
}

#[test]
fn dynamic_style_re_runs_on_signal_change() {
    with_recorder_and_owner(|log| {
        let (color, set_color) = signal("red".to_string());
        let css = computed(move || format!("background: {};", color.get()));
        let _h = render! {
            XStyled(style: css)
        };
        set_color.set("blue".into());
        flush();
        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetStyles { css, .. } => Some(css.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            styles,
            vec![
                "background: red;".to_string(),
                "background: blue;".to_string()
            ]
        );
    });
}

#[test]
fn non_style_props_route_through_set_attribute_with_kebab_case() {
    // `value` and `placeholder` are regular SetAttribute calls.
    // Snake-case prop name → kebab-case attribute name (matches the
    // built-in `attr()` mapping in __tags). For these two props the
    // names are already single-word so kebab == snake.
    with_recorder_and_owner(|log| {
        let _h = render! {
            XInput(value: "hello", placeholder: "type here")
        };
        let attrs: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } => Some((key.clone(), value.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            attrs,
            vec![
                ("value".to_string(), "hello".to_string()),
                ("placeholder".to_string(), "type here".to_string()),
            ]
        );
    });
}

#[test]
fn read_signal_prop_tracks_underlying_signal() {
    with_recorder_and_owner(|log| {
        let (value, set_value) = signal("alpha".to_string());
        let _h = render! {
            XInput(value: value, placeholder: "static")
        };
        set_value.set("beta".into());
        flush();
        set_value.set("gamma".into());
        flush();
        let value_sets: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "value" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(value_sets, vec!["alpha", "beta", "gamma"]);
    });
}
