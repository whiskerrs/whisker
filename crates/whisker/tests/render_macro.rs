//! Integration test for the `render!` macro.
//!
//! Covers the compose-syntax surface: static elements + attrs, event
//! handlers, builder-shaped text content (`Text(value: …)`), dynamic
//! attribute closures, `Show` / `For` control flow.
//!
//! Tests install a small recording renderer, expand `render!`,
//! and assert on the recorded op sequence.

use std::cell::RefCell;
use std::rc::Rc;
use whisker::flush;
use whisker::prelude::*;
use whisker::runtime::reactive::{__reset_for_tests, Owner};
use whisker::runtime::view::{
    BindType, DynRenderer, Element, install_renderer, uninstall_renderer,
};

#[derive(Debug, Clone, PartialEq)]
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
        declarations: usize,
    },
    SetId {
        id: u32,
        value: String,
    },
    SetDataset {
        id: u32,
        entries: usize,
    },
    SetAccessibility {
        id: u32,
        label: Option<String>,
    },
    SetTextMaxLines {
        id: u32,
        value: u32,
    },
    SetObject {
        id: u32,
        key: String,
    },
    Append {
        parent: u32,
        child: u32,
    },
    Event {
        id: u32,
        name: String,
        bind_type: BindType,
    },
}

#[derive(Default)]
struct Recorder {
    next: ::std::cell::Cell<u32>,
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
    fn release_element(&self, _h: Element) {}
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
            declarations: style.len(),
        });
        true
    }
    fn set_element_id(&self, h: Element, value: String) {
        self.log.borrow_mut().push(Op::SetId { id: h.id(), value });
    }
    fn set_dataset(&self, h: Element, value: Dataset) {
        self.log.borrow_mut().push(Op::SetDataset {
            id: h.id(),
            entries: value.iter().count(),
        });
    }
    fn set_accessibility(&self, h: Element, value: Accessibility) {
        self.log.borrow_mut().push(Op::SetAccessibility {
            id: h.id(),
            label: value.label,
        });
    }
    fn set_text_max_lines(&self, h: Element, value: u32) {
        self.log
            .borrow_mut()
            .push(Op::SetTextMaxLines { id: h.id(), value });
    }
    fn set_attribute_object(&self, h: Element, key: &str, _value: &[(String, f64)]) {
        self.log.borrow_mut().push(Op::SetObject {
            id: h.id(),
            key: key.to_owned(),
        });
    }
    fn append_child(&self, p: Element, c: Element) {
        self.log.borrow_mut().push(Op::Append {
            parent: p.id(),
            child: c.id(),
        });
    }
    fn remove_child(&self, _p: Element, _c: Element) {}
    fn set_event_listener(
        &self,
        h: Element,
        name: &str,
        bind_type: BindType,
        _cb: Box<dyn Fn(whisker::WhiskerValue) + 'static>,
    ) {
        self.log.borrow_mut().push(Op::Event {
            id: h.id(),
            name: name.into(),
            bind_type,
        });
    }
    fn set_root(&self, _p: Element) {}
    fn flush(&self) {}
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
    let owner = Owner::new(None);
    let result = owner.with(|| f(log));
    owner.dispose();
    uninstall_renderer(prev);
    result
}

struct PlainValue;

struct PlainValueBuilder {
    value: i32,
}

impl PlainValue {
    fn builder() -> PlainValueBuilder {
        PlainValueBuilder { value: 0 }
    }
}

impl PlainValueBuilder {
    fn value(mut self, value: i32) -> Self {
        self.value = value;
        self
    }

    fn build(self) -> i32 {
        self.value
    }
}

// ----- Static element trees -------------------------------------------------

#[test]
fn single_view_emits_create_and_returns_handle() {
    with_recorder(|log| {
        let h = render! { View() };
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
fn compose_is_the_generic_adapter_over_the_same_builder_chain() {
    with_recorder(|log| {
        let h = compose! { View(id: "root") };
        assert_eq!(h.id(), 0);
        assert_eq!(
            *log.borrow(),
            vec![
                Op::Create {
                    id: 0,
                    tag: ElementTag::View,
                },
                Op::SetId {
                    id: 0,
                    value: "root".into(),
                },
            ]
        );
    });
}

#[test]
fn compose_does_not_special_case_whisker_elements() {
    let value = compose! { PlainValue(value: 42) };
    assert_eq!(value, 42);
}

#[test]
fn builtin_builders_are_a_complete_public_non_macro_api() {
    with_recorder(|log| {
        let root = View::builder()
            .id("root")
            .body(|body| {
                body.push(Text::builder().value("Hello").build());
            })
            .build();
        assert_eq!(root.id(), 0);
        assert!(log.borrow().iter().any(|op| matches!(
            op,
            Op::Append {
                parent: 0,
                child: 1
            }
        )));
    });
}

#[test]
fn nested_view_with_text_child() {
    with_recorder(|log| {
        let _h = render! {
            View {
                Text(value: "Hello")
            }
        };
        // Expected ops:
        //  Create View (0)
        //  Create Text (1)
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
        // The raw_text's text attr is set in an effect, so its order
        // relative to the append is an implementation detail of
        // `value()`.
        assert!(ops.iter().any(|op| matches!(op, Op::SetAttr {
            id: 2, key, value
        } if key == "text" && value == "Hello")));
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::Append {
                parent: 1,
                child: 2
            }
        )));
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::Append {
                parent: 0,
                child: 1
            }
        )));
    });
}

#[test]
fn style_attribute_emits_structured_style() {
    with_recorder(|log| {
        let _ = render! {
            View(style: css!(padding: px(16)))
        };
        let ops = log.borrow();
        assert_eq!(
            ops[0],
            Op::Create {
                id: 0,
                tag: ElementTag::View
            }
        );
        assert!(ops.contains(&Op::SetSpecifiedStyle {
            id: 0,
            declarations: 4,
        }));
    });
}

#[test]
fn common_metadata_uses_structured_renderer_paths() {
    with_recorder(|log| {
        let _ = render! {
            View(
                id: "banner",
                dataset: Dataset::new().int("index", 3),
                accessibility: Accessibility::new().label("Example"),
            )
        };
        let ops = log.borrow();
        assert_eq!(
            ops[0],
            Op::Create {
                id: 0,
                tag: ElementTag::View
            }
        );
        assert!(ops.contains(&Op::SetId {
            id: 0,
            value: "banner".into(),
        }));
        assert!(ops.contains(&Op::SetDataset { id: 0, entries: 1 }));
        assert!(ops.contains(&Op::SetAccessibility {
            id: 0,
            label: Some("Example".into()),
        }));
    });
}

#[test]
fn built_in_control_options_route_to_typed_setters() {
    let has = |ops: &[Op], key: &str, val: &str| {
        ops.iter()
            .any(|op| matches!(op, Op::SetAttr { key: k, value: v, .. } if k == key && v == val))
    };

    with_recorder(|log| {
        let _ = render! {
            ScrollView(
                axis: ScrollAxis::Horizontal,
                scroll_enabled: false,
                snap: ScrollSnap::center(),
            )
        };
        let ops = log.borrow();
        assert!(has(&ops, "scroll-orientation", "horizontal"), "got {ops:?}");
        assert!(has(&ops, "enable-scroll", "false"), "got {ops:?}");
        assert!(ops.contains(&Op::SetObject {
            id: 0,
            key: "item-snap".into()
        }));
    });

    with_recorder(|log| {
        let _ = render! {
            Text(value: "hi", max_lines: 2_u32)
        };
        let ops = log.borrow();
        assert!(ops.contains(&Op::SetTextMaxLines { id: 0, value: 2 }));
    });
}

#[test]
fn on_tap_emits_set_event_listener() {
    with_recorder(|log| {
        let fired = Rc::new(RefCell::new(false));
        let f = fired.clone();
        let _ = render! {
            View(on_tap: move |_| *f.borrow_mut() = true)
        };
        let ops = log.borrow();
        assert!(ops.iter().any(|op| matches!(
            op,
            Op::Event { name, bind_type, .. } if name == "tap" && *bind_type == BindType::Bind
        )));
        // The recorder stores but never fires the callback;
        // registration is all the macro layer can be held to.
        assert!(!*fired.borrow());
    });
}

#[test]
fn tap_propagation_variants_route_to_bind_types() {
    // Each `on_[capture_]tap[_catch]` kwarg registers a "tap" listener
    // with the matching propagation `BindType`.
    with_recorder(|log| {
        let _ = render! {
            View {
                View(on_tap: |_| {})
                View(on_tap_catch: |_| {})
                View(on_capture_tap: |_| {})
                View(on_capture_tap_catch: |_| {})
            }
        };
        let kinds: Vec<BindType> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::Event {
                    name, bind_type, ..
                } if name == "tap" => Some(*bind_type),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                BindType::Bind,
                BindType::Catch,
                BindType::CaptureBind,
                BindType::CaptureCatch,
            ]
        );
    });
}

#[test]
fn component_specific_events_route_bind_only() {
    // Tag-specific events use the binding declared by their public builder.
    with_recorder(|log| {
        let _ = render! {
            ScrollView(on_scroll: |_| {})
        };
        let names: Vec<(String, BindType)> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::Event {
                    name, bind_type, ..
                } => Some((name.clone(), *bind_type)),
                _ => None,
            })
            .collect();
        assert!(
            names.contains(&("scroll".to_owned(), BindType::Bind)),
            "missing bind listener for scroll; got {names:?}"
        );
    });
}

#[test]
fn multiple_children_append_in_order() {
    with_recorder(|log| {
        let _ = render! {
            View {
                Text(value: "A")
                Text(value: "B")
                Text(value: "C")
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

// ----- Dynamic value interpolation via `Text(value: …)` --------------------

#[test]
fn dynamic_value_renders_initial_via_effect() {
    with_recorder_and_owner(|log| {
        let (count, _set_count) = signal(0_i32).split();
        // The macro does not auto-wrap kwargs: reactive numeric →
        // string interpolation goes through a `computed`, which reaches
        // the `value` builder as `Signal::Dynamic`.
        let label = computed(move || count.get().to_string());
        let _h = render! {
            Text(value: label)
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
        let (count, set_count) = signal(0_i32).split();
        let label = computed(move || count.get().to_string());
        let _h = render! {
            Text(value: label)
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
        let (color, set_color) = signal(NamedColor::Red).split();
        let css = computed(move || Css::new().color(Color::Named(color.get())));
        let _h = render! {
            View(style: css)
        };
        set_color.set(NamedColor::Blue);
        flush();

        let styles: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetSpecifiedStyle { declarations, .. } => Some(*declarations),
                _ => None,
            })
            .collect();
        assert_eq!(styles, vec![1, 1]);
    });
}

#[test]
fn dynamic_element_id_re_runs_on_dep_change() {
    with_recorder_and_owner(|log| {
        let (id, set_id) = signal("first".to_string()).split();
        let _h = render! {
            View(id: id)
        };
        set_id.set("second".into());
        flush();

        let ids: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetId { value, .. } => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec!["first", "second"]);
    });
}

#[test]
fn static_value_only_sets_text_once() {
    with_recorder_and_owner(|log| {
        let _h = render! {
            Text(value: "static")
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
fn text_value_combines_static_and_dynamic_content() {
    with_recorder_and_owner(|log| {
        let (count, _set) = signal(7_i32).split();
        let count_label = computed(move || format!("count={}", count.get()));
        let _h = render! {
            Text(value: count_label)
        };
        let set_text: Vec<_> = log
            .borrow()
            .iter()
            .filter_map(|op| match op {
                Op::SetAttr { key, value, .. } if key == "text" => Some(value.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(set_text, vec!["count=7".to_string()]);
    });
}

#[test]
fn signal_only_updates_elements_that_read_it() {
    with_recorder_and_owner(|log| {
        let (a, set_a) = signal(0_i32).split();
        let (b, _set_b) = signal(100_i32).split();
        let a_label = computed(move || a.get().to_string());
        let b_label = computed(move || b.get().to_string());
        let _h = render! {
            View {
                Text(value: a_label)
                Text(value: b_label)
            }
        };
        log.borrow_mut().clear(); // ignore initial ops
        set_a.set(1);
        flush();
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
        let (cond, _set) = signal(true).split();
        let _h = render! {
            View {
                Show(when: move || cond.get()) {
                    Text(value: "main")
                }
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
        let (cond, _set) = signal(false).split();
        let _h = render! {
            View {
                Show(
                    when: move || cond.get(),
                    fallback: || render! { Text(value: "fallback") },
                ) {
                    Text(value: "main")
                }
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
        let (cond, set_cond) = signal(true).split();
        let _h = render! {
            View {
                Show(
                    when: move || cond.get(),
                    fallback: || render! { Text(value: "fb") },
                ) {
                    Text(value: "main")
                }
            }
        };
        log.borrow_mut().clear();
        set_cond.set(false);
        flush();

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
        let (cond, _set) = signal(false).split();
        let _h = render! {
            View {
                Show(when: move || cond.get()) {
                    Text(value: "only")
                }
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
        ])
        .split();
        let _h = render! {
            View {
                ForEach(
                    each: move || items.get(),
                    key: |i: &Item| i.id,
                    children: move |i: Item| render! { Text(value: i.name) },
                )
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
        assert_eq!(
            texts,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    });
}

#[test]
fn for_adds_new_items_on_update() {
    with_recorder_and_owner(|log| {
        let (items, set_items) = signal(vec![1_u32, 2]).split();
        let _h = render! {
            View {
                ForEach(
                    each: move || items.get(),
                    key: |x: &u32| *x,
                    children: move |x: u32| render! { Text(value: x.to_string()) },
                )
            }
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
        let (items, set_items) = signal(vec![1_u32, 2, 3]).split();
        let _h = render! {
            View {
                ForEach(
                    each: move || items.get(),
                    key: |x: &u32| *x,
                    children: move |x: u32| render! { Text(value: x.to_string()) },
                )
            }
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
        let (items, set_items) = signal(vec![1_u32, 2, 3]).split();
        let _h = render! {
            View {
                ForEach(
                    each: move || items.get(),
                    key: |x: &u32| *x,
                    children: move |x: u32| render! { Text(value: x.to_string()) },
                )
            }
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
fn view_scroll_view_tags_supported() {
    with_recorder(|log| {
        let _ = render! {
            View {
                ScrollView {
                    View()
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
            vec![ElementTag::View, ElementTag::ScrollView, ElementTag::View]
        );
    });
}
