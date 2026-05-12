//! End-to-end tests for `rsx!`. We compile the macro into actual
//! `Element` values and assert on their structure.

use lyra::prelude::*;
use lyra::rsx;

#[test]
fn empty_view() {
    let tree = rsx! { view {} };
    assert_eq!(tree.tag, ElementTag::View);
    assert!(tree.children.is_empty());
}

#[test]
fn view_with_style_attribute() {
    let tree = rsx! {
        view { style: "color: red;" }
    };
    assert_eq!(tree.styles, "color: red;");
}

#[test]
fn view_with_arbitrary_attribute() {
    let tree = rsx! {
        view { class: "row" }
    };
    assert_eq!(tree.get_attr("class"), Some("row"));
}

#[test]
fn view_with_key() {
    let tree = rsx! {
        view { key: "row-7" }
    };
    assert_eq!(tree.key.as_deref(), Some("row-7"));
}

#[test]
fn nested_text_with_string_literal() {
    let tree = rsx! {
        text {
            "Hello, world"
        }
    };
    assert_eq!(tree.tag, ElementTag::Text);
    assert_eq!(tree.children.len(), 1);
    assert_eq!(tree.children[0].tag, ElementTag::RawText);
    assert_eq!(tree.children[0].get_attr("text"), Some("Hello, world"));
}

#[test]
fn page_with_view_with_text() {
    let tree = rsx! {
        page {
            view {
                text {
                    "Lyra"
                }
            }
        }
    };
    assert_eq!(tree.tag, ElementTag::Page);
    assert_eq!(tree.children[0].tag, ElementTag::View);
    assert_eq!(tree.children[0].children[0].tag, ElementTag::Text);
}

#[test]
fn multiple_attributes_and_children() {
    let tree = rsx! {
        view {
            class: "row",
            style: "padding: 16px;",
            text { "a" }
            text { "b" }
        }
    };
    assert_eq!(tree.get_attr("class"), Some("row"));
    assert_eq!(tree.styles, "padding: 16px;");
    assert_eq!(tree.children.len(), 2);
}

#[test]
fn dynamic_text_via_brace_block() {
    let name = "World";
    let tree = rsx! {
        text {
            { format!("Hello, {name}") }
        }
    };
    assert_eq!(tree.children[0].get_attr("text"), Some("Hello, World"));
}

#[test]
fn dynamic_attribute_value() {
    let cls = String::from("dynamic");
    let tree = rsx! {
        view { class: cls }
    };
    assert_eq!(tree.get_attr("class"), Some("dynamic"));
}

#[test]
fn on_event_handler_camel_case() {
    let tree = rsx! {
        view {
            onClick: || { /* no-op */ }
        }
    };
    assert_eq!(tree.events.len(), 1);
    assert_eq!(tree.events[0].name, "click");
}

#[test]
fn on_event_handler_snake_case() {
    let tree = rsx! {
        view {
            on_tap: || { /* no-op */ }
        }
    };
    assert_eq!(tree.events.len(), 1);
    assert_eq!(tree.events[0].name, "tap");
}

#[test]
fn deeply_nested_tree() {
    let tree = rsx! {
        page {
            view {
                view {
                    view {
                        text {
                            "deep"
                        }
                    }
                }
            }
        }
    };
    let mut depth = 0;
    let mut cur = &tree;
    while !cur.children.is_empty() {
        depth += 1;
        cur = &cur.children[0];
    }
    assert_eq!(depth, 5, "page > 3*view > text > raw_text");
}

#[test]
fn many_children_via_loop_pattern() {
    let items = (0..3)
        .map(|i| rsx! { text { { format!("item {i}") } } })
        .collect::<Vec<_>>();
    let tree = view().children(items);
    assert_eq!(tree.children.len(), 3);
    assert_eq!(
        tree.children[2].children[0].get_attr("text"),
        Some("item 2"),
    );
}

#[test]
fn image_element_compiles() {
    let tree = rsx! {
        image { src: "icon.png" }
    };
    assert_eq!(tree.tag, ElementTag::Image);
    assert_eq!(tree.get_attr("src"), Some("icon.png"));
}
