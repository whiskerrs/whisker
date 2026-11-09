use whisker::css::{Border, BorderStyle};
use whisker::prelude::*;
use whisker_input::Input;
use whisker_safe_area::safe_area_insets;

#[derive(Clone, PartialEq)]
struct Todo {
    id: u32,
    title: String,
    done: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Active,
    Done,
}

const BG: u32 = 0x0B0B0F;
const CARD: u32 = 0x1A1A22;
const ACCENT: u32 = 0x7C5CFF;
const MUTED: u32 = 0x8A8A99;
const WHITE: u32 = 0xFFFFFF;

fn row() -> Css {
    Css::new()
        .display(Display::Flex)
        .flex_direction(FlexDirection::Row)
        .align_items(AlignItems::Center)
}

fn column() -> Css {
    Css::new()
        .display(Display::Flex)
        .flex_direction(FlexDirection::Column)
}

fn label(color: u32) -> Css {
    Css::new().font_size(px(16)).color(Color::hex(color))
}

#[whisker::main]
fn app() -> Element {
    render! {
        TodoApp
    }
}

#[component]
fn todo_app() -> Element {
    let todos = signal(vec![
        Todo { id: 1, title: "Read the Whisker docs".into(), done: true },
        Todo { id: 2, title: "Build a todo app".into(), done: false },
        Todo { id: 3, title: "Compare with NSH".into(), done: false },
    ]);
    let next_id = signal(4_u32);
    let draft = signal(String::new());
    let filter = signal(Filter::All);
    let insets = safe_area_insets();

    let submit = move || {
        let title = draft.get_untracked().trim().to_owned();
        if title.is_empty() {
            return;
        }
        let id = next_id.get_untracked();
        next_id.set(id + 1);
        todos.update(|list| list.push(Todo { id, title, done: false }));
        draft.set(String::new());
    };
    let visible = computed(move || {
        let f = filter.get();
        todos.with(|list| {
            list.iter()
                .filter(|t| match f {
                    Filter::All => true,
                    Filter::Active => !t.done,
                    Filter::Done => t.done,
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let left = computed(move || todos.with(|l| l.iter().filter(|t| !t.done).count()));

    render! {
        View(
            style: computed(move || {
                column()
                    .flex_grow(1.0)
                    .background_color(Color::hex(BG))
                    .padding_top(px(insets.get().top as f32 + 16.0))
                    .padding_bottom(px(insets.get().bottom as f32 + 16.0))
                    .padding_left(px(16))
                    .padding_right(px(16))
                    .gap(px(12))
            }),
        ) {
            View(style: row().justify_content(JustifyContent::SpaceBetween)) {
                Text(value: "Todos", style: Css::new().font_size(px(28)).color(Color::hex(WHITE)))
                Text(value: computed(move || format!("{} left", left.get())), style: label(MUTED))
            }
            View(style: row().gap(px(8))) {
                Input(
                    text: draft,
                    placeholder: "What needs doing?",
                    on_submit: move |_: String| submit(),
                    style: label(WHITE)
                        .flex_grow(1.0)
                        .height(px(44))
                        .padding(px(10))
                        .border_radius(px(8))
                        .background_color(Color::hex(CARD)),
                )
                View(
                    on_tap: move |_| submit(),
                    style: Css::new()
                        .padding((px(10), px(16)))
                        .border_radius(px(8))
                        .background_color(Color::hex(ACCENT)),
                ) {
                    Text(value: "Add", style: label(WHITE))
                }
            }
            View(style: row().gap(px(8))) {
                FilterTab(text: "All", value: Filter::All, filter: filter)
                FilterTab(text: "Active", value: Filter::Active, filter: filter)
                FilterTab(text: "Done", value: Filter::Done, filter: filter)
            }
            List(
                each: move || visible.get(),
                key: |t: &Todo| t.id,
                children: move |item: ReadSignal<Todo>| render! {
                    TodoRow(item: item, todos: todos)
                },
                content_style: column().row_gap(px(8)),
                style: column().flex_grow(1.0).min_height(px(0)),
            )
        }
    }
}

#[component]
fn filter_tab(text: &'static str, value: Filter, filter: RwSignal<Filter>) -> Element {
    render! {
        View(
            on_tap: move |_| filter.set(value),
            style: computed(move || {
                row()
                    .flex_grow(1.0)
                    .justify_content(JustifyContent::Center)
                    .padding((px(8), px(0)))
                    .border_radius(px(8))
                    .background_color(Color::hex(if filter.get() == value { ACCENT } else { CARD }))
            }),
        ) {
            Text(value: text, style: label(WHITE))
        }
    }
}

#[component]
fn todo_row(item: ReadSignal<Todo>, todos: RwSignal<Vec<Todo>>) -> Element {
    let id = item.with_untracked(|t| t.id);
    render! {
        View(
            style: row()
                .gap(px(12))
                .padding((px(12), px(16)))
                .border_radius(px(10))
                .background_color(Color::hex(CARD)),
        ) {
            View(
                on_tap: move |_| {
                    todos.update(|list| {
                        if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                            t.done = !t.done;
                        }
                    })
                },
                style: row().gap(px(12)).flex_grow(1.0),
            ) {
                View(
                    style: computed(move || {
                        Css::new()
                            .width(px(22))
                            .height(px(22))
                            .border_radius(px(11))
                            .border(Border::new().width(px(2)).style(BorderStyle::Solid).color(Color::hex(ACCENT)))
                            .background_color(Color::hex(if item.get().done { ACCENT } else { CARD }))
                    }),
                )
                Text(
                    value: computed(move || item.get().title),
                    style: computed(move || {
                        label(if item.get().done { MUTED } else { WHITE }).flex_grow(1.0)
                    }),
                )
            }
            View(
                on_tap: move |_| todos.update(|list| list.retain(|t| t.id != id)),
                style: Css::new().padding((px(0), px(8))),
            ) {
                Text(value: "Delete", style: Css::new().font_size(px(14)).color(Color::hex(MUTED)))
            }
        }
    }
}
