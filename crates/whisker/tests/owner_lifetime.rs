use whisker::runtime::{RuntimeContext, RuntimeWakeHandle};
use whisker::{Owner, RwSignal};
#[test]
fn back_handler_does_not_run_after_its_owner_is_disposed() {
    let runtime = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
    runtime.enter(|| {
        let owner = Owner::new(None);
        let guard = owner.with(|| {
            let value = RwSignal::new(1);
            whisker::back::on_back(move || {
                let _ = value.get();
            })
        });
        owner.dispose();
        assert!(!whisker::back::dispatch());
        drop(guard);
    });
}
