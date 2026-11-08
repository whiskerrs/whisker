// One test per binary: it replaces the process-wide panic hook.

use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;

use whisker_runtime::reactive::{Owner, RwSignal, Signal, StoredValue};

static LOCATION: Mutex<Option<(String, u32)>> = Mutex::new(None);

fn panic_line<R>(read: impl FnOnce() -> R) -> u32 {
    *LOCATION.lock().unwrap() = None;
    assert!(panic::catch_unwind(AssertUnwindSafe(read)).is_err());
    let (file, line) = LOCATION.lock().unwrap().take().unwrap();
    assert_eq!(file, file!());
    line
}

#[test]
fn reading_a_disposed_signal_panics_at_the_caller() {
    panic::set_hook(Box::new(|info| {
        let location = info.location().unwrap();
        *LOCATION.lock().unwrap() = Some((location.file().to_owned(), location.line()));
    }));

    let owner = Owner::new(None);
    let (rw, stored) = owner.with(|| (RwSignal::new(1), StoredValue::new(1)));
    let read = rw.read_only();
    let prop: Signal<i32> = read.into();
    owner.dispose();

    assert_eq!(panic_line(|| read.get()), line!());
    assert_eq!(panic_line(|| read.get_untracked()), line!());
    assert_eq!(panic_line(|| read.with(|_| ())), line!());
    assert_eq!(panic_line(|| read.with_untracked(|_| ())), line!());
    assert_eq!(panic_line(|| rw.get()), line!());
    assert_eq!(panic_line(|| rw.get_untracked()), line!());
    assert_eq!(panic_line(|| rw.with(|_| ())), line!());
    assert_eq!(panic_line(|| rw.with_untracked(|_| ())), line!());
    assert_eq!(panic_line(|| prop.get()), line!());
    assert_eq!(panic_line(|| stored.get()), line!());
    assert_eq!(panic_line(|| stored.with(|_| ())), line!());
    assert_eq!(panic_line(|| stored.update(|_| ())), line!());
    assert_eq!(panic_line(|| stored.set(2)), line!());

    let _ = panic::take_hook();
}
