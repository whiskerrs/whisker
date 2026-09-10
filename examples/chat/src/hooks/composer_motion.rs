use whisker::prelude::*;
use whisker::{AnimConfig, AnimationController};

#[derive(Clone, Copy)]
pub struct ComposerMotion {
    pub focused: RwSignal<bool>,
    pub progress: ReadSignal<f32>,
}

pub fn use_composer_motion() -> ComposerMotion {
    let focused = signal(false);
    let controller = AnimationController::new(AnimConfig::ease_in_out(640));
    let progress = controller.value();
    effect(move || controller.animate_to(if focused.get() { 1.0 } else { 0.0 }));
    ComposerMotion { focused, progress }
}

#[cfg(test)]
mod tests {
    use super::*;
    use whisker::Owner;
    use whisker::runtime::{RuntimeContext, RuntimeWakeHandle, anim_hook, reactive};

    #[test]
    fn focus_reversal_is_continuous_and_blur_hides_the_action() {
        let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
        runtime.enter(|| {
            let owner = Owner::new(None);
            owner.with(|| {
                let motion = use_composer_motion();
                motion.focused.set(true);
                reactive::flush();
                anim_hook::step(0.0);
                anim_hook::step(240.0);
                let midway = motion.progress.get_untracked();
                assert!(midway > 0.0 && midway < 1.0);
                motion.focused.set(false);
                reactive::flush();
                assert_eq!(motion.progress.get_untracked(), midway);
                motion.focused.set(true);
                reactive::flush();
                anim_hook::step(260.0);
                anim_hook::step(940.0);
                assert_eq!(motion.progress.get_untracked(), 1.0);
                motion.focused.set(false);
                reactive::flush();
                anim_hook::step(960.0);
                anim_hook::step(1640.0);
                assert_eq!(motion.progress.get_untracked(), 0.0);
            });
            owner.dispose();
            anim_hook::step(1700.0);
        });
    }
}
