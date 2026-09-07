use whisker::runtime::{RuntimeContext, RuntimeWakeHandle};
#[test]
fn keyboard_height_can_be_read_after_runtime_recreation() {
    let first = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
    assert_eq!(
        first.enter(|| whisker_keyboard::keyboard_height().get()),
        0.0
    );
    first.shutdown();
    let second = RuntimeContext::new(RuntimeWakeHandle::new(|| {}));
    assert_eq!(
        second.enter(|| whisker_keyboard::keyboard_height().get()),
        0.0
    );
}
