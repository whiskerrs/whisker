use whisker::prelude::*;
use whisker::{AnimConfig, AnimationController};

#[derive(Clone, Copy)]
pub struct SidebarState {
    pub open: RwSignal<bool>,
    pub progress: ReadSignal<f32>,
    pub visible: ReadSignal<bool>,
    mounted: RwSignal<bool>,
}

impl SidebarState {
    pub fn mount_panel(self) {
        on_mount(move || self.mounted.set(true));
        on_cleanup(move || self.mounted.set(false));
    }
}

pub fn use_sidebar(initially_open: bool) -> SidebarState {
    let open = signal(initially_open);
    let mounted = signal(false);
    let controller = AnimationController::new(AnimConfig::ease_out(220));
    controller.set_value(if initially_open { 1.0 } else { 0.0 });
    let progress = controller.value();
    effect(move || {
        if !open.get() {
            controller.animate_to(0.0);
        } else if mounted.get() {
            controller.animate_to(1.0);
        }
    });
    SidebarState {
        open,
        progress,
        visible: computed(move || open.get() || progress.get() > 0.0),
        mounted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker::Owner;
    use whisker::runtime::{RuntimeContext, RuntimeWakeHandle, anim_hook, reactive};

    fn frame(timestamp: f64) {
        anim_hook::step(timestamp);
        reactive::flush();
    }

    #[test]
    fn opening_keeps_its_duration_after_a_slow_mount_frame() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            owner.with(|| {
                let state = use_sidebar(false);
                for start in [0.0, 1000.0] {
                    state.open.set(true);
                    reactive::flush();
                    let panel = Owner::new(Some(owner));
                    panel.with(|| state.mount_panel());
                    reactive::flush_mounts();
                    frame(start);
                    frame(start + 310.0);
                    assert_eq!(state.progress.get_untracked(), 0.0);
                    frame(start + 326.0);
                    assert!((0.0..1.0).contains(&state.progress.get_untracked()));
                    assert!(state.progress.get_untracked() > 0.0);
                    frame(start + 530.0);
                    assert_eq!(state.progress.get_untracked(), 1.0);

                    state.open.set(false);
                    reactive::flush();
                    frame(start + 600.0);
                    frame(start + 650.0);
                    assert!((0.0..1.0).contains(&state.progress.get_untracked()));
                    assert!(state.visible.get_untracked());
                    frame(start + 820.0);
                    assert!(!state.visible.get_untracked());
                    panel.dispose();
                    reactive::flush();
                }
            });
            owner.dispose();
        });
    }
}
