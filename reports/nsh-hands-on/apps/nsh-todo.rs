use nsh_base::{button, input};
use nsh_core::prelude::*;

#[native_module("nsh-todo/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

#[derive(Clone, PartialEq)]
struct Todo {
    id: u32,
    title: SharedString,
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

#[component]
pub fn App() -> impl IntoElement {
    let todos = signal(vec![
        Todo { id: 1, title: "Read the NSH docs".into(), done: true },
        Todo { id: 2, title: "Build a todo app".into(), done: false },
        Todo { id: 3, title: "Compare with Whisker".into(), done: false },
    ]);
    let next_id = signal(4_u32);
    let draft = signal(String::new());
    let filter = signal(Filter::All);

    let submit = move || {
        let title = draft.get_untracked().trim().to_owned();
        if title.is_empty() {
            return;
        }
        let id = next_id.get_untracked();
        next_id.set(id + 1);
        todos.update(|list| list.push(Todo { id, title: title.into(), done: false }));
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

    let filter_tab = move |label: &'static str, value: Filter| {
        button()
            .flex_1()
            .items_center()
            .py_2()
            .rounded(px(8.0))
            .bg(move || if filter.get() == value { rgb(ACCENT) } else { rgb(CARD) })
            .on_press(move || filter.set(value))
            .child(text(label).text_color(rgb(0xFFFFFF)))
    };

    let row = move |item: ReadSignal<Todo>, _index: ReadSignal<usize>| {
        let id = item.with_untracked(|t| t.id);
        view()
            .flex_row()
            .items_center()
            .gap_3()
            .px_4()
            .py_3()
            .rounded(px(10.0))
            .bg(rgb(CARD))
            .child(
                button()
                    .flex_row()
                    .flex_1()
                    .items_center()
                    .gap_3()
                    .on_press(move || {
                        todos.update(|list| {
                            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                                t.done = !t.done;
                            }
                        })
                    })
                    .child(
                        view()
                            .w(px(22.0))
                            .h(px(22.0))
                            .rounded_full()
                            .border_2()
                            .border_color(rgb(ACCENT))
                            .bg(move || if item.get().done { rgb(ACCENT) } else { rgb(CARD) }),
                    )
                    .child(
                        text(move || item.get().title)
                            .flex_1()
                            .text_color(move || if item.get().done { rgb(MUTED) } else { rgb(0xFFFFFF) }),
                    ),
            )
            .child(
                button()
                    .px_2()
                    .on_press(move || todos.update(|list| list.retain(|t| t.id != id)))
                    .child(text("×").text_size(px(20.0)).text_color(rgb(MUTED))),
            )
    };

    view()
        .flex_1()
        .bg(rgb(BG))
        .p_safe_area()
        .child(
            view()
                .flex_1()
                .gap_3()
                .p_4()
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(text("Tasks").text_size(px(28.0)).text_color(rgb(0xFFFFFF)))
                        .child(text(move || format!("{} left", left.get())).text_color(rgb(MUTED))),
                )
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            input()
                                .value(move || draft.get())
                                .on_change(move |value: &str| draft.set(value.to_owned()))
                                .on_submit(move |_: &str| submit())
                                .placeholder("What needs doing?")
                                .flex_1()
                                .px_3()
                                .py_2()
                                .rounded(px(8.0))
                                .bg(rgb(CARD))
                                .text_color(rgb(0xFFFFFF)),
                        )
                        .child(
                            button()
                                .px_4()
                                .py_2()
                                .rounded(px(8.0))
                                .bg(rgb(ACCENT))
                                .active(|s| s.opacity(0.6))
                                .on_press(submit)
                                .child(text("Add").text_color(rgb(0xFFFFFF))),
                        ),
                )
                .child(
                    view()
                        .flex_row()
                        .gap_2()
                        .child(filter_tab("All", Filter::All))
                        .child(filter_tab("Active", Filter::Active))
                        .child(filter_tab("Done", Filter::Done)),
                )
                .child(
                    scroll()
                        .flex_1()
                        .child(view().gap_2().child(each(visible, |t: &Todo| t.id, row))),
                ),
        )
}
