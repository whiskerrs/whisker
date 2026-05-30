//! Phase 7-Φ.C end-to-end test: a `#[component]` declared with a
//! `Signal<T>` prop receives a parent's reactive value and tracks
//! it through to the underlying element's attribute.
//!
//! This is the user-facing assertion of the unified reactivity
//! model:
//!
//! - Parent passes a `ReadSignal<String>` (or `RwSignal<String>`,
//!   or a `String`, or a `&str`) to a child component prop typed
//!   `Signal<String>`.
//! - The child's body reads the prop inside a `computed` /
//!   `effect`, so the underlying signal is registered as a
//!   dependency.
//! - When the parent updates its signal, the child's element
//!   updates via the effect chain — same fine-grained reactivity
//!   that built-in tags already enjoy.

use std::cell::RefCell;
use std::rc::Rc;

use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, create_owner, dispose_owner};
use whisker::runtime::view::{install_renderer, uninstall_renderer, DynRenderer, Element};
use whisker::{flush, with_owner};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Create { id: u32, tag: ElementTag },
    SetAttr { id: u32, key: String, value: String },
    SetStyles { id: u32, css: String },
    Append { parent: u32, child: u32 },
    Remove { parent: u32, child: u32 },
    Event { id: u32, name: String },
    SetRoot { id: u32 },
    Flush,
    Release { id: u32 },
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
    fn release_element(&mut self, h: Element) {
        self.log.borrow_mut().push(Op::Release { id: h.id() });
    }
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
    fn remove_child(&mut self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Remove {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn set_event_listener(
        &mut self,
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
    fn set_root(&mut self, p: Element) {
        self.log.borrow_mut().push(Op::SetRoot { id: p.id() });
    }
    fn flush(&mut self) {
        self.log.borrow_mut().push(Op::Flush);
    }
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

// ----- The component under test ------------------------------------------

/// Component with one `Signal<String>` prop. Body reads the prop
/// inside a `computed` to drive a reactive `style` attribute.
#[component]
fn colored_tile(color: Signal<String>) -> Element {
    // `color` is owned by the outer `__hot::call(move || …)` wrap
    // the `#[component]` macro generates for hot-patch dispatch.
    // Moving it into the `computed` closure would consume the
    // outer FnMut's capture, hence the .clone() — cheap on a
    // Dynamic Signal (Copy of the underlying ReadSignal NodeId)
    // and a String clone on the Static arm.
    let style = {
        let color = color.clone();
        computed(move || format!("background: {};", color.get()))
    };
    render! {
        view(style: style)
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
                Op::SetStyles { css, .. } => Some(css.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(styles, vec!["background: red;".to_string()]);
    });
}

#[test]
fn read_signal_prop_tracks_underlying_signal() {
    with_recorder_and_owner(|log| {
        let (color, set_color) = signal("red".to_string());
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
                Op::SetStyles { css, .. } => Some(css.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            styles,
            vec![
                "background: red;".to_string(),
                "background: blue;".to_string(),
                "background: green;".to_string(),
            ]
        );
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
                Op::SetStyles { css, .. } => Some(css.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            styles,
            vec![
                "background: orange;".to_string(),
                "background: purple;".to_string(),
            ]
        );
    });
}

// ----- Show + Resource-like state flip (the hn-reader pattern) ----------

#[test]
fn show_flips_when_signal_holding_option_transitions() {
    // Reproduces the hn-reader pattern: a `Show` whose `when` reads
    // a signal that goes from `None` (fallback shown) to `Some(_)`
    // (children shown). Catches any reactivity regression in the
    // Show + signal-read chain that the simpler `text(value: …)`
    // tests don't exercise.
    with_recorder_and_owner(|log| {
        let (state, set_state) = signal::<Option<&'static str>>(None);
        // Wrap in `view` so the wrapper-less `Show` has a parent to
        // anchor against (the phantom anchor never reaches Lynx, so
        // the log only records the inner branch's ops).
        let _h = render! {
            view {
                Show(
                    when: move || state.get().is_some(),
                    fallback: move || render! { ColoredTile(color: "loading") },
                ) {
                    ColoredTile(color: "loaded")
                }
            }
        };
        // Initial: fallback mounted → "loading" attribute set.
        let initial_styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetStyles { css, .. } => Some(css.clone()),
                _ => None,
            })
            .collect();
        assert!(
            initial_styles.iter().any(|s| s == "background: loading;"),
            "initial render must mount the fallback branch (got styles {initial_styles:?})"
        );

        // Flip the signal — Show effect should re-run and swap to
        // the children branch.
        set_state.set(Some("done"));
        flush();
        let after_styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetStyles { css, .. } => Some(css.clone()),
                _ => None,
            })
            .collect();
        assert!(
            after_styles.iter().any(|s| s == "background: loaded;"),
            "after set_state to Some, the children branch must be mounted \
             (this is the regression hn-reader hit: Loading banner never \
             swapped because Show's reactivity broke). styles seen: {after_styles:?}"
        );
    });
}

#[test]
fn computed_prop_tracks_chain_of_signals() {
    with_recorder_and_owner(|log| {
        let (count, set_count) = signal(0_i32);
        // Caller-side computed → ReadSignal<String> → flows into
        // ColoredTile as Signal::Dynamic. Updates to `count`
        // propagate end-to-end.
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
                Op::SetStyles { css, .. } => Some(css.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            styles,
            vec![
                "background: even;".to_string(),
                "background: odd;".to_string(),
                "background: even;".to_string(),
            ]
        );
    });
}
