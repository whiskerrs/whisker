//! End-to-end test: a `#[component]` declared with a `Signal<T>` prop
//! receives a parent's reactive value and tracks it through to the
//! underlying element's attribute.
//!
//! The user-facing assertion of the unified reactivity model:
//!
//! - Parent passes a `ReadSignal<String>` (or `RwSignal<String>`,
//!   or a `String`, or a `&str`) to a child component prop typed
//!   `Signal<String>`.
//! - The child's body reads the prop inside a `computed` /
//!   `effect`, so the underlying signal is registered as a
//!   dependency.
//! - When the parent updates its signal, the child's element updates
//!   via the effect chain — the same fine-grained reactivity built-in
//!   tags get.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::flush;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{DynRenderer, Element, install_renderer, uninstall_renderer};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create {
        id: u32,
        tag: ElementTag,
    },
    SetAttr {
        id: u32,
        key: String,
        value: String,
    },
    SetSpecifiedStyle {
        id: u32,
        style: whisker_engine::whisker_style::SpecifiedStyle,
    },
    Append {
        parent: u32,
        child: u32,
    },
    Remove {
        parent: u32,
        child: u32,
    },
    Event {
        id: u32,
        name: String,
    },
    SetRoot {
        id: u32,
    },
    Flush,
    Release {
        id: u32,
    },
}

#[derive(Default)]
struct Recorder {
    next: ::std::cell::Cell<u32>,
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
    fn create_element(&self, tag: ElementTag) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create { id, tag });
        Element::from_raw(id)
    }
    fn create_element_by_name(&self, _tag_name: &str) -> Element {
        let id = self.next.get();
        self.next.set(id + 1);
        self.log.borrow_mut().push(Op::Create {
            id,
            tag: ElementTag::View,
        });
        Element::from_raw(id)
    }
    fn release_element(&self, h: Element) {
        self.log.borrow_mut().push(Op::Release { id: h.id() });
    }
    fn set_attribute(&self, h: Element, k: &str, v: &str) {
        self.log.borrow_mut().push(Op::SetAttr {
            id: h.id(),
            key: k.into(),
            value: v.into(),
        });
    }
    fn set_specified_style(
        &self,
        h: Element,
        style: &whisker_engine::whisker_style::SpecifiedStyle,
    ) -> bool {
        self.log.borrow_mut().push(Op::SetSpecifiedStyle {
            id: h.id(),
            style: style.clone(),
        });
        true
    }
    fn append_child(&self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Remove {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn set_event_listener(
        &self,
        h: Element,
        name: &str,
        _bind_type: whisker::runtime::view::BindType,
        _cb: Box<dyn Fn(whisker::WhiskerValue) + 'static>,
    ) {
        self.log.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
        });
    }
    fn set_root(&self, p: Element) {
        self.log.borrow_mut().push(Op::SetRoot { id: p.id() });
    }
    fn flush(&self) {
        self.log.borrow_mut().push(Op::Flush);
    }
}

fn with_recorder_and_owner<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    __reset_for_tests();
    let (rec, log) = Recorder::with_log();
    let prev = install_renderer(Box::new(rec));
    let owner = Owner::new(None);
    let out = owner.with(|| f(log));
    owner.dispose();
    uninstall_renderer(prev);
    out
}

// ----- The component under test ------------------------------------------

/// Component with one `Signal<String>` prop. Body reads the prop inside a
/// `computed` to drive a reactive structured `style` attribute.
#[component]
fn colored_tile(color: Signal<String>) -> Element {
    // `Signal<T>` is `Copy`, so `color` moves into the `computed`
    // closure without a `.clone()` even though the `#[component]` body
    // is the `FnMut` the macro wraps for hot-patch dispatch.
    let style = computed(move || {
        let width = color.get().bytes().map(f32::from).sum::<f32>();
        Css::new().width(px(width))
    });
    render! {
        View(style: style)
    }
}

// ----- Tests ----------------------------------------------------------------

#[test]
fn static_string_prop_sets_attribute_once() {
    with_recorder_and_owner(|log| {
        let _h = render! {
            ColoredTile(color: "red")
        };
        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(styles.len(), 1);
        assert_eq!(styles[0].len(), 1);
    });
}

#[test]
fn read_signal_prop_tracks_underlying_signal() {
    with_recorder_and_owner(|log| {
        let (color, set_color) = signal("red".to_string()).split();
        let _h = render! {
            ColoredTile(color: color)
        };
        set_color.set("blue".into());
        flush();
        set_color.set("green".into());
        flush();
        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(styles.len(), 3);
        assert_ne!(styles[0], styles[1]);
        assert_ne!(styles[1], styles[2]);
    });
}

#[test]
fn rw_signal_prop_tracks_underlying_signal() {
    with_recorder_and_owner(|log| {
        let color = RwSignal::new("orange".to_string());
        let _h = render! {
            ColoredTile(color: color)
        };
        color.set("purple".into());
        flush();
        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(styles.len(), 2);
        assert_ne!(styles[0], styles[1]);
    });
}

// ----- Show + Resource-like state flip --------------------------------

#[test]
fn show_flips_when_signal_holding_option_transitions() {
    // A `Show` whose `when` reads a signal going from `None` (fallback)
    // to `Some(_)` (children) — the Show + signal-read chain the
    // simpler `Text(value: …)` tests don't reach.
    with_recorder_and_owner(|log| {
        let (state, set_state) = signal::<Option<&'static str>>(None).split();
        let _h = render! {
            Show(
                when: move || state.get().is_some(),
                fallback: move || render! { ColoredTile(color: "loading") },
            ) {
                ColoredTile(color: "loaded")
            }
        };
        let initial_styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(initial_styles.len(), 1, "fallback branch must be styled");

        set_state.set(Some("done"));
        flush();
        let after_styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect();
        assert!(
            after_styles.len() > initial_styles.len()
                && after_styles.last() != initial_styles.last(),
            "after set_state to Some, the children branch must be mounted \
             (this is the regression hn-reader hit: Loading banner never \
             swapped because Show's reactivity broke). styles seen: {after_styles:?}"
        );
    });
}

#[test]
fn computed_prop_tracks_chain_of_signals() {
    with_recorder_and_owner(|log| {
        let (count, set_count) = signal(0_i32).split();
        let color_label = computed(move || {
            if count.get() % 2 == 0 {
                "even".to_string()
            } else {
                "odd".to_string()
            }
        });
        let _h = render! {
            ColoredTile(color: color_label)
        };
        set_count.set(1);
        flush();
        set_count.set(2);
        flush();
        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { style, .. } => Some(style.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(styles.len(), 3);
        assert_eq!(styles[0], styles[2]);
        assert_ne!(styles[0], styles[1]);
    });
}
