use std::cell::Cell;
use whisker::event::ScrollEvent;
use whisker::prelude::*;

#[derive(Clone, Copy)]
pub struct ChatScroll {
    pub following: RwSignal<bool>,
    pub at_end: RwSignal<bool>,
    pub changed: Callback<ScrollEvent>,
}

pub fn use_chat_scroll(initially_empty: bool) -> ChatScroll {
    let following = signal(initially_empty);
    let at_end = signal(initially_empty);
    let previous_offset = Cell::new(0.0);
    let changed = Callback::new(move |event: ScrollEvent| {
        let detail = event.detail;
        let end = detail.scroll_height - detail.viewport_height - detail.scroll_top < 48.0;
        let previous = previous_offset.replace(detail.scroll_top);
        at_end.set(end);
        // Derive direction from offsets: not every Host supplies drag metadata.
        if end || detail.is_dragging || detail.scroll_top < previous {
            following.set(end);
        }
    });
    ChatScroll {
        following,
        at_end,
        changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker::Owner;

    #[test]
    fn web_offsets_control_following_without_drag_or_delta_fields() {
        let runtime =
            whisker::runtime::RuntimeContext::new(whisker::runtime::RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            owner.with(|| {
                let scroll = use_chat_scroll(true);
                let event = |top| {
                    serde_json::from_value(serde_json::json!({
                        "type": "scroll",
                        "detail": {"scrollTop": top, "scrollHeight": 2000, "viewportHeight": 800}
                    }))
                    .unwrap()
                };
                scroll.changed.run(event(1200));
                assert!(scroll.at_end.get_untracked());
                scroll.changed.run(event(900));
                assert!(!scroll.at_end.get_untracked());
                assert!(!scroll.following.get_untracked());
                scroll.changed.run(event(1100));
                assert!(!scroll.following.get_untracked());
                scroll.changed.run(event(1200));
                assert!(scroll.following.get_untracked());
            });
            owner.dispose();
        });
    }
}
