use super::{button::Button, navigation, theme};
use crate::state::AppState;
use whisker::prelude::*;
use whisker_router::{Outlet, use_navigator};

#[component]
pub fn startup() -> Element {
    let app = use_context::<AppState>().expect("AppState context");
    let nav = use_navigator();
    let restored = signal(None::<Result<(), String>>);
    let restore = Callback::new(move |()| {
        let result = app.restore().and_then(|()| {
            if app.connection().get_untracked().is_none()
                && app.turns().with_untracked(Vec::is_empty)
            {
                navigation::start_setup(&nav)?;
            }
            Ok(())
        });
        restored.set(Some(result));
    });
    on_mount(move || restore.call());
    render! {
        View(style: theme::fill()) {
            Show(when: move || restored.with(Option::is_none)) {
                Text(value: "Whisker Chat — Loading…", style: theme::text(20.0).padding(px(24)))
            }
            Show(when: move || restored.with(|result| matches!(result, Some(Err(_))))) {
                View(style: theme::column().padding(px(24)).gap(px(16))) {
                    Text(
                        value: computed(move || {
                            restored.with(|result| match result {
                                Some(Err(error)) => error.clone(),
                                _ => String::new(),
                            })
                        }),
                        style: theme::muted(),
                    )
                    Button(
                        label: "Try loading again",
                        on_press: restore,
                    )
                }
            }
            Show(when: move || restored.with(|result| matches!(result, Some(Ok(()))))) {
                Outlet()
            }
        }
    }
}
