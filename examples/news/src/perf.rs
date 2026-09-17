//! Opt-in, wall-clock paced workload using the same public APIs as the UI.

use super::*;
use std::cell::RefCell;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use whisker_router::RouterHandle;

struct Workload {
    list: ListHandle<usize>,
    nav: RouterHandle,
    state: NewsState,
    article: Option<ListHandle<usize>>,
    phase: String,
}

thread_local! {
    static WORKLOAD: RefCell<Option<Workload>> = const { RefCell::new(None) };
}

pub fn article_scroll(handle: ListHandle<usize>) {
    WORKLOAD.with_borrow_mut(|slot| {
        if let Some(workload) = slot {
            workload.article = Some(handle);
        }
    });
    on_cleanup(|| {
        WORKLOAD.with_borrow_mut(|slot| {
            if let Some(workload) = slot {
                workload.article = None;
            }
        })
    });
}

fn unix_seconds() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() / 1000.0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }
}

fn log(line: String) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&line.into());
    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!("{line}");
        if let Ok(path) = std::env::var("WHISKER_NEWS_PHASE_FILE") {
            use std::io::Write;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}

fn mark(phase: &str) {
    let seconds = unix_seconds();
    log(format!(
        "NEWS_PERF {{\"phase\":\"{phase}\",\"unix_seconds\":{seconds}}}"
    ));
}

pub fn start(list: ListHandle<usize>, nav: RouterHandle, state: NewsState) {
    #[cfg(target_arch = "wasm32")]
    let enabled = web_sys::window()
        .unwrap()
        .location()
        .search()
        .unwrap_or_default()
        .contains("autorun=1");
    #[cfg(not(target_arch = "wasm32"))]
    let enabled = std::env::var("WHISKER_NEWS_AUTORUN").as_deref() == Ok("1");
    if !enabled {
        return;
    }
    let installed = WORKLOAD.with_borrow_mut(|slot| {
        if slot.is_some() {
            return false;
        }
        *slot = Some(Workload {
            list,
            nav,
            state,
            article: None,
            phase: String::new(),
        });
        true
    });
    if !installed {
        return;
    }
    let dispatcher = whisker::runtime::runtime_dispatcher().expect("mounted runtime");
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::{JsCast, closure::Closure};
        let start = js_sys::Date::now();
        let callback = Closure::<dyn FnMut()>::new(move || {
            let elapsed = (js_sys::Date::now() - start) / 1000.0;
            if elapsed < 47.0 {
                dispatcher.post(move || step(elapsed));
            }
        });
        let window = web_sys::window().unwrap();
        let timer = window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                8,
            )
            .unwrap();
        on_cleanup(move || {
            window.clear_interval_with_handle(timer);
            drop(callback);
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let pending = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        on_cleanup(move || {
            cancellation.store(true, Ordering::Release);
            WORKLOAD.with_borrow_mut(|slot| *slot = None);
        });
        std::thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed().as_secs_f64() < 47.0 && !cancelled.load(Ordering::Acquire) {
                if !pending.swap(true, Ordering::AcqRel) {
                    let pending = pending.clone();
                    if !dispatcher.post(move || {
                        step(start.elapsed().as_secs_f64());
                        pending.store(false, Ordering::Release);
                    }) {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_micros(8333));
            }
        });
    }
}

fn step(elapsed: f64) {
    // Release the TLS borrow before navigation mounts another route.
    let Some(mut workload) = WORKLOAD.with_borrow_mut(Option::take) else {
        return;
    };
    let (phase, cycle, local) = if elapsed < 2.5 {
        ("settle", 0, elapsed)
    } else if elapsed < 39.7 {
        let t = elapsed - 2.5;
        let cycle = (t / 12.4) as usize;
        let local = t % 12.4;
        let phase = if local < 6.0 {
            "feed_scroll"
        } else if local < 6.7 {
            "push"
        } else if local < 10.7 {
            "article_scroll"
        } else if local < 11.4 {
            "pop"
        } else if local < 11.9 {
            "filter"
        } else {
            "filter_reset"
        };
        (phase, cycle, local)
    } else if elapsed < 40.9 {
        ("saved_push", 0, 0.0)
    } else if elapsed < 45.9 {
        ("recovery", 0, 0.0)
    } else {
        ("complete", 0, 0.0)
    };
    let key = format!("{phase}_{cycle}");
    let changed = key != workload.phase;
    if changed {
        mark(&key);
        workload.phase = key;
    }
    let nav = workload.nav.clone();
    let state = workload.state;
    if phase == "feed_scroll" {
        let y = if local < 3.0 {
            local * 1800.0
        } else {
            (6.0 - local) * 1800.0
        };
        workload
            .list
            .scroll_to(ListScrollTarget::Offset(y), ScrollBehavior::Instant)
            .expect("mounted feed");
    } else if phase == "article_scroll" {
        workload
            .article
            .as_ref()
            .expect("mounted article")
            .scroll_to(
                ListScrollTarget::Offset((local - 6.7) * 900.0),
                ScrollBehavior::Instant,
            )
            .expect("mounted article list");
    } else if changed && phase == "recovery" {
        workload
            .list
            .scroll_to(ListScrollTarget::Start, ScrollBehavior::Instant)
            .unwrap();
    }
    WORKLOAD.with_borrow_mut(|slot| *slot = Some(workload));
    if changed {
        match phase {
            "push" => {
                nav.push(&format!("/article/{cycle}")).unwrap();
            }
            "pop" => {
                toggle_saved(state, cycle);
                nav.back().unwrap();
            }
            "filter" => state.category.set(cycle + 1),
            "filter_reset" => state.category.set(0),
            "saved_push" => {
                nav.push("/saved").unwrap();
            }
            "recovery" => {
                nav.back().unwrap();
            }
            _ => {}
        }
    }
}

/// First successful decoded image callback, not GPU presentation completion.
pub fn first_image_loaded() {
    static LOADED: AtomicBool = AtomicBool::new(false);
    if !LOADED.swap(true, Ordering::Relaxed) {
        let seconds = unix_seconds();
        log(format!("NEWS_IMAGE_LOAD {{\"unix_seconds\":{seconds}}}"));
    }
}
