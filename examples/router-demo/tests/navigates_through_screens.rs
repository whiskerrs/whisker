//! Integration test: drive the router-demo app through several
//! navigation events and verify the recording renderer sees the
//! expected text content at each step.
//!
//! Mirrors the shape of `examples/counter/tests/counter_renders.rs`.
//! The point isn't pixel-perfect equivalence — it's that:
//!  - `#[route]` generates a working `Route` impl,
//!  - `RouteStack::push/back/replace_all` flow through `Router`'s
//!    effect into actual mount / unmount calls on the renderer.

use std::cell::RefCell;
use std::rc::Rc;

use router_demo::{render_with, AppRoute};
use whisker::flush;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, create_owner, with_owner};
use whisker::runtime::view::{install_renderer, uninstall_renderer, DynRenderer, Element};
use whisker_router::{route::Route, route_stack};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    SetAttr { id: u32, key: String, value: String },
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
    fn create_element(&mut self, _tag: ElementTag) -> Element {
        let id = self.next;
        self.next += 1;
        Element::from_raw(id)
    }
    fn create_element_by_name(&mut self, _tag_name: &str) -> Element {
        let id = self.next;
        self.next += 1;
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
    fn set_inline_styles(&mut self, _h: Element, _css: &str) {}
    fn append_child(&mut self, _p: Element, _c: Element) {}
    fn remove_child(&mut self, _p: Element, _c: Element) {}
    fn set_event_listener(
        &mut self,
        _h: Element,
        _name: &str,
        _bind_type: whisker::runtime::view::BindType,
        _cb: Box<dyn Fn(whisker::WhiskerValue) + 'static>,
    ) {
    }
    fn set_root(&mut self, _p: Element) {}
    fn flush(&mut self) {}
}

fn texts(log: &[Op]) -> Vec<String> {
    log.iter()
        .filter_map(|Op::SetAttr { key, value, .. }| match key.as_str() {
            "text" => Some(value.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn route_macro_round_trips() {
    // Pure macro check — no renderer needed.
    assert_eq!(AppRoute::Home.to_path(), "/");
    assert_eq!(AppRoute::Post { id: 7 }.to_path(), "/post/7");
    assert_eq!(
        AppRoute::parse("/post/12").unwrap(),
        AppRoute::Post { id: 12 }
    );
    assert_eq!(AppRoute::parse("/list").unwrap(), AppRoute::List);
}

#[test]
fn initial_mount_shows_home() {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let nav = route_stack(AppRoute::Home);
    let _root = with_owner(owner, || render_with(nav.clone()));

    let ts = texts(&log.borrow());
    assert!(ts.contains(&"Home".to_string()), "got {ts:?}");
    assert!(!ts.contains(&"List".to_string()), "got {ts:?}");

    uninstall_renderer(None);
}

#[test]
fn push_swaps_to_target_screen() {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let nav = route_stack(AppRoute::Home);
    let _root = with_owner(owner, || render_with(nav.clone()));

    log.borrow_mut().clear();
    nav.push(AppRoute::List);
    flush();

    let ts = texts(&log.borrow());
    assert!(ts.contains(&"List".to_string()), "got {ts:?}");

    uninstall_renderer(None);
}

#[test]
fn nested_push_with_param_renders_post() {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let nav = route_stack(AppRoute::Home);
    let _root = with_owner(owner, || render_with(nav.clone()));

    nav.push(AppRoute::List);
    flush();
    log.borrow_mut().clear();

    nav.push(AppRoute::Post { id: 7 });
    flush();

    let ts = texts(&log.borrow());
    assert!(ts.contains(&"Post #7".to_string()), "got {ts:?}");

    uninstall_renderer(None);
}

#[test]
fn back_returns_to_previous_screen() {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let nav = route_stack(AppRoute::Home);
    let _root = with_owner(owner, || render_with(nav.clone()));

    nav.push(AppRoute::List);
    flush();
    nav.push(AppRoute::Post { id: 3 });
    flush();
    log.borrow_mut().clear();

    let popped = nav.back();
    flush();

    assert!(popped, "expected back() to report a pop");
    let ts = texts(&log.borrow());
    // After popping the Post screen we should be back on List.
    assert!(ts.contains(&"List".to_string()), "got {ts:?}");

    uninstall_renderer(None);
}

#[test]
fn replace_all_drops_history() {
    __reset_for_tests();
    let (rec, _log) = Recorder::new();
    let _prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);

    let nav = route_stack(AppRoute::Home);
    let _root = with_owner(owner, || render_with(nav.clone()));

    nav.push(AppRoute::List);
    flush();
    nav.push(AppRoute::Post { id: 1 });
    flush();
    nav.push(AppRoute::Settings);
    flush();

    nav.replace_all(AppRoute::Home);
    flush();

    // Only one entry left, calling back() again is a no-op.
    assert!(!nav.back(), "back() at root must return false");

    uninstall_renderer(None);
}
