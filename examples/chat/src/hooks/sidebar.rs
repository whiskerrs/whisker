use whisker::prelude::*;
use whisker::{AnimConfig, AnimationController};

#[derive(Clone, Copy)]
pub struct SidebarState {
    pub open: RwSignal<bool>,
    pub progress: ReadSignal<f32>,
    pub visible: ReadSignal<bool>,
}

pub fn use_sidebar(initially_open: bool) -> SidebarState {
    let open = signal(initially_open);
    let controller = AnimationController::new(AnimConfig::ease_out(220));
    controller.set_value(if initially_open { 1.0 } else { 0.0 });
    let progress = controller.value();
    effect(move || controller.animate_to(if open.get() { 1.0 } else { 0.0 }));
    SidebarState {
        open,
        progress,
        visible: computed(move || open.get() || progress.get() > 0.0),
    }
}
