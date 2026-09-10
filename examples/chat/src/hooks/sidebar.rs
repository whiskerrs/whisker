use whisker::prelude::*;
use whisker::{AnimConfig, AnimationController};

#[derive(Clone, Copy)]
pub struct SidebarState {
    pub open: RwSignal<bool>,
    pub progress: ReadSignal<f32>,
    pub visible: ReadSignal<bool>,
    pub compact: ReadSignal<bool>,
    width: RwSignal<f32>,
    mounted: RwSignal<bool>,
}

impl SidebarState {
    pub fn observe_container(self, element: Element) {
        whisker::runtime::view::observe_layout(
            element,
            Box::new(move |layout| self.width.set(layout.geometry.border_box.width)),
        );
    }

    pub fn mount_panel(self) {
        on_mount(move || self.mounted.set(true));
        on_cleanup(move || self.mounted.set(false));
    }
}

pub fn use_sidebar() -> SidebarState {
    let open = signal(false);
    let mounted = signal(false);
    let width = signal(0.0);
    let compact = computed(move || {
        cfg!(any(target_os = "ios", target_os = "android"))
            || width.get() < crate::design::size::SIDEBAR_BREAKPOINT
    });
    let controller = AnimationController::new(AnimConfig::ease_out(220));
    let progress = controller.value();
    effect(move || {
        if compact.get() {
            open.set(false);
            controller.set_value(0.0);
        } else if !open.get() {
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
        compact,
        width,
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
    fn compact_resize_closes_the_sidebar_without_reopening_it_on_widening() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            owner.with(|| {
                let state = use_sidebar();
                reactive::flush();
                assert!(state.compact.get_untracked());
                state.width.set(768.0);
                reactive::flush();
                assert!(!state.compact.get_untracked());
                state.mounted.set(true);
                state.open.set(true);
                reactive::flush();
                frame(0.0);
                frame(300.0);
                assert!(state.visible.get_untracked());
                state.width.set(767.0);
                reactive::flush();
                assert!(state.compact.get_untracked());
                assert!(!state.open.get_untracked());
                assert_eq!(state.progress.get_untracked(), 0.0);
                assert!(!state.visible.get_untracked());
                state.width.set(1200.0);
                reactive::flush();
                assert!(!state.compact.get_untracked());
                assert!(!state.visible.get_untracked());
            });
            owner.dispose();
        });
    }

    #[test]
    fn opening_keeps_its_duration_after_a_slow_mount_frame() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            owner.with(|| {
                let state = use_sidebar();
                state.width.set(1200.0);
                reactive::flush();
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
