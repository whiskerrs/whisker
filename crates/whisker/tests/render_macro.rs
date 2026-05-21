//! Integration test for the `render!` macro.
//!
//! Covers the compose-syntax surface: static elements + attrs, event
//! handlers, builder-shaped text content (`text(value: …)`), dynamic
//! attribute closures, `Show` / `For` control flow.
//!
//! Tests install a small recording renderer, expand `render!`,
//! and assert on the recorded op sequence.

use std::cell::RefCell;
use std::rc::Rc;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, create_owner, dispose_owner};
use whisker::runtime::view::{install_renderer, uninstall_renderer, DynRenderer, ElementHandle};
use whisker::{flush, with_owner};

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

fn with_recorder<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    let (rec, log) = Recorder::new();
    let prev = install_renderer(Box::new(rec));
    let result = f(log);
    uninstall_renderer(prev);
    result
}

fn with_recorder_and_owner<R>(f: impl FnOnce(Rc<RefCell<Vec<Op>>>) -> R) -> R {
    __reset_for_tests();
    let (rec, log) = Recorder::new();
    let prev = install_renderer(Box::new(rec));
    let owner = create_owner(None);
    let result = with_owner(owner, || f(log));
    dispose_owner(owner);
    uninstall_renderer(prev);
    result
}

// ----- Static element trees -------------------------------------------------

#[test]
fn single_view_emits_create_and_returns_handle() {
    with_recorder(|log| {
        let h = render! { view() };
        assert_eq!(h.id(), 0);
        assert_eq!(
            *log.borrow(),
            vec![Op::Create {
                id: 0,
                tag: ElementTag::View
            }]
        );
    });
}

#[test]
fn nested_view_with_text_child() {
    with_recorder(|log| {
        let _h = render! {
            view {
                text(value: "Hello")
            }
        };
        // Expected ops:
        //  Create view (0)
        //  Create text (1)
        //  Create raw_text (2)  ← from text's `value` method
        //  Append raw_text → text
        //  SetAttr text="Hello" on raw_text (in the effect)
        //  Append text → view
        let ops = log.borrow();
        assert_eq!(
            ops[0],
            Op::Create {
                id: 0,
                tag: ElementTag::View
            }
        );
        assert_eq!(
            ops[1],
            Op::Create {
                id: 1,
                tag: ElementTag::Text
            }
        );
        assert_eq!(
            ops[2],
            Op::Create {
                id: 2,
                tag: ElementTag::RawText
            }
        );
        // The raw_text gets its text attr set in an effect. We check
        // the attr appears in the op stream and the raw_text is
        // attached to the text. Order between the SetAttr and the
        // append is an implementation detail of the value() method.
        assert!(ops.iter().any(|op| matches!(op, Op::SetAttr {
            id: 2, key, value
        } if key == "text" && value == "Hello")));
        assert!(ops.iter().any(|op| matches!(op, Op::Append {
            parent: 1, child: 2
        })));
        assert!(ops.iter().any(|op| matches!(op, Op::Append {
            parent: 0, child: 1
        })));
    });
}

#[test]
fn style_attribute_emits_set_inline_styles() {
    with_recorder(|log| {
        let _ = render! {
            view(style: "padding: 16px;")
        };
        let ops = log.borrow();
        assert_eq!(
            ops[0],
            Op::Create {
                id: 0,
                tag: ElementTag::View
            }
        );
        assert!(ops.contains(&Op::SetStyles {
            id: 0,
            css: "padding: 16px;".into()
        }));
    });
}

#[test]
fn arbitrary_attribute_emits_set_attribute() {
    with_recorder(|log| {
        let _ = render! {
            image(
                src: "https://example.com/x.png",
                alt: "example",
            )
        };
        let ops = log.borrow();
        assert_eq!(
            ops[0],
            Op::Create {
                id: 0,
                tag: ElementTag::Image
            }
        );
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
            view(on_tap: move || *f.borrow_mut() = true)
        };
        let ops = log.borrow();
        assert!(ops
            .iter()
            .any(|op| matches!(op, Op::Event { name, .. } if name == "tap")));
        // The recorder stored but doesn't fire the callback —
        // verifying registration is enough at the macro layer.
        assert!(!*fired.borrow());
    });
}

#[test]
fn camel_case_event_handler_lowercased() {
    with_recorder(|log| {
        let _ = render! {
            view(onTap: || {})
        };
        let ops = log.borrow();
        assert!(ops
            .iter()
            .any(|op| matches!(op, Op::Event { name, .. } if name == "tap")));
    });
}

#[test]
fn multiple_children_append_in_order() {
    with_recorder(|log| {
        let _ = render! {
            view {
                text(value: "A")
                text(value: "B")
                text(value: "C")
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
        let appends_to_view: Vec<_> = appends.iter().filter(|(p, _)| *p == 0).collect();
        assert_eq!(appends_to_view.len(), 3);
    });
}

// ----- Dynamic value interpolation via `text(value: …)` --------------------

#[test]
fn dynamic_value_renders_initial_via_effect() {
    with_recorder_and_owner(|log| {
        let (count, _set_count) = signal(0_i32);
        let _h = render! {
            text(value: count.get())
        };
        let set_text: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(set_text, vec!["0".to_string()]);
    });
}

#[test]
fn dynamic_value_updates_on_signal_write() {
    with_recorder_and_owner(|log| {
        let (count, set_count) = signal(0_i32);
        let _h = render! {
            text(value: count.get())
        };
        set_count.set(5);
        flush();
        set_count.set(42);
        flush();

        let set_text: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(set_text, vec!["0", "5", "42"]);
    });
}

#[test]
fn dynamic_style_re_runs_on_dep_change() {
    with_recorder_and_owner(|log| {
        let (color, set_color) = signal("red".to_string());
        let _h = render! {
            view(style: format!("color: {};", color.get()))
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
        assert_eq!(styles, vec!["color: red;", "color: blue;"]);
    });
}

#[test]
fn dynamic_attribute_re_runs_on_dep_change() {
    with_recorder_and_owner(|log| {
        let (src, set_src) = signal("a.png".to_string());
        let _h = render! {
            image(src: src.get())
        };
        set_src.set("b.png".into());
        flush();

        let attrs: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "src" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(attrs, vec!["a.png", "b.png"]);
    });
}

#[test]
fn static_value_only_sets_text_once() {
    with_recorder_and_owner(|log| {
        let _h = render! {
            text(value: "static")
        };
        let set_text: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(set_text, vec!["static".to_string()]);
    });
}

#[test]
fn mixed_static_and_dynamic_children_via_raw_text() {
    // Two separate raw_text siblings — first static, second
    // reading the signal. Same op-stream shape the legacy
    // `<text>"count=" {count.get()}</text>` produced.
    with_recorder_and_owner(|log| {
        let (count, _set) = signal(7_i32);
        let _h = render! {
            text {
                raw_text(text: "count=")
                raw_text(text: count.get())
            }
        };
        let set_text: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(set_text, vec!["count=".to_string(), "7".to_string()]);
    });
}

#[test]
fn signal_only_updates_elements_that_read_it() {
    with_recorder_and_owner(|log| {
        let (a, set_a) = signal(0_i32);
        let (b, _set_b) = signal(100_i32);
        let _h = render! {
            view {
                text(value: a.get())
                text(value: b.get())
            }
        };
        log.borrow_mut().clear(); // ignore initial ops
        set_a.set(1);
        flush();
        // Only the element reading `a` should have its SetAttr fire.
        let set_text_count = log
            .borrow()
            .iter()
            .filter(|op| matches!(op, Op::SetAttr { key, .. } if key == "text"))
            .count();
        assert_eq!(
            set_text_count, 1,
            "only the a-reading raw_text should update"
        );
    });
}

// ----- Show + For control flow --------------------------------------------

#[test]
fn show_renders_children_when_true() {
    with_recorder_and_owner(|log| {
        let (cond, _set) = signal(true);
        let _h = render! {
            Show(when: move || cond.get()) {
                text(value: "main")
            }
        };
        let texts: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["main".to_string()]);
    });
}

#[test]
fn show_renders_fallback_when_false() {
    with_recorder_and_owner(|log| {
        let (cond, _set) = signal(false);
        let _h = render! {
            Show(
                when: move || cond.get(),
                fallback: || render! { text(value: "fallback") },
            ) {
                text(value: "main")
            }
        };
        let texts: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["fallback".to_string()]);
    });
}

#[test]
fn show_swaps_on_condition_flip() {
    with_recorder_and_owner(|log| {
        let (cond, set_cond) = signal(true);
        let _h = render! {
            Show(
                when: move || cond.get(),
                fallback: || render! { text(value: "fb") },
            ) {
                text(value: "main")
            }
        };
        log.borrow_mut().clear();
        set_cond.set(false);
        flush();

        // After flip: a new "fb" element gets created + text set.
        let texts_after: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts_after, vec!["fb".to_string()]);
    });
}

#[test]
fn show_without_fallback_renders_nothing_when_false() {
    with_recorder_and_owner(|log| {
        let (cond, _set) = signal(false);
        let _h = render! {
            Show(when: move || cond.get()) {
                text(value: "only")
            }
        };
        let texts: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert!(
            texts.is_empty(),
            "no children should mount when when=false and no fallback"
        );
    });
}

#[test]
fn for_renders_initial_items() {
    with_recorder_and_owner(|log| {
        #[derive(Clone)]
        struct Item {
            id: u32,
            name: &'static str,
        }
        let (items, _set_items) = signal(vec![
            Item { id: 1, name: "a" },
            Item { id: 2, name: "b" },
            Item { id: 3, name: "c" },
        ]);
        let _h = render! {
            For(
                each: move || items.get(),
                key: |i: &Item| i.id,
                children: move |i: Item| render! { text(value: i.name) },
            )
        };

        let texts: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            texts,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    });
}

#[test]
fn for_adds_new_items_on_update() {
    with_recorder_and_owner(|log| {
        let (items, set_items) = signal(vec![1_u32, 2]);
        let _h = render! {
            For(
                each: move || items.get(),
                key: |x: &u32| *x,
                children: move |x: u32| render! { text(value: x.to_string()) },
            )
        };
        log.borrow_mut().clear();

        set_items.set(vec![1, 2, 3, 4]);
        flush();

        let new_texts: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert!(
            new_texts.contains(&"3".to_string()),
            "item 3 must be rendered"
        );
        assert!(
            new_texts.contains(&"4".to_string()),
            "item 4 must be rendered"
        );
        assert!(
            !new_texts.contains(&"1".to_string()),
            "item 1 must NOT be re-rendered"
        );
    });
}

#[test]
fn for_reorders_existing_items_visually() {
    with_recorder_and_owner(|log| {
        let (items, set_items) = signal(vec![1_u32, 2, 3]);
        let _h = render! {
            For(
                each: move || items.get(),
                key: |x: &u32| *x,
                children: move |x: u32| render! { text(value: x.to_string()) },
            )
        };
        log.borrow_mut().clear();

        set_items.set(vec![3, 2, 1]);
        flush();

        let appends_to_wrapper = log
            .borrow()
            .iter()
            .filter(|op| matches!(op, Op::Append { parent: 0, .. }))
            .count();
        assert!(
            appends_to_wrapper >= 3,
            "expected re-attach for reordered items; got {appends_to_wrapper}"
        );
    });
}

#[test]
fn for_removes_items_on_update() {
    with_recorder_and_owner(|log| {
        let (items, set_items) = signal(vec![1_u32, 2, 3]);
        let _h = render! {
            For(
                each: move || items.get(),
                key: |x: &u32| *x,
                children: move |x: u32| render! { text(value: x.to_string()) },
            )
        };
        log.borrow_mut().clear();

        set_items.set(vec![2]);
        flush();

        let new_texts: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert!(new_texts.is_empty(), "no new SetAttr for survived items");
    });
}

#[test]
fn page_view_image_scroll_view_tags_supported() {
    with_recorder(|log| {
        let _ = render! {
            page {
                scroll_view {
                    image()
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
