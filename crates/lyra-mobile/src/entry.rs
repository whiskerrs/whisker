//! FFI entry points the host (Swift `LyraView` on iOS, Kotlin
//! `LyraView` on Android) calls to drive the Rust runtime.
//!
//! State of the demo as of A0/A1:
//! - `lyra_mobile_app_main` runs once at startup, dispatches to the Lynx
//!   TASM thread, and creates a persistent `Runtime` whose root signal
//!   is the demo counter.
//! - `lyra_mobile_tick` is called by the Swift `Timer` (~30 Hz). Each
//!   tick increments the counter signal so the rendered text changes
//!   every frame, exercising the full reactive loop end-to-end on iOS.
//!
//! Tap-driven event delivery (A2/A3) is wired through the Renderer
//! trait but not yet flowing on iOS — see `bridge/src/lyra_bridge.mm`'s
//! `lyra_bridge_set_event_listener` for the Lynx-side limitation that
//! still needs to be unblocked.

use crate::app_logic::counter_render_tree;
use crate::bridge_ffi::{lyra_bridge_dispatch, LyraEngine};
use crate::bridge_renderer::BridgeRenderer;
use lyra_runtime::element::Element;
use lyra_runtime::runtime::Runtime;
use lyra_runtime::signal::{use_signal, Signal};
use std::cell::RefCell;
use std::ffi::c_void;

struct AppState {
    runtime: Runtime<BridgeRenderer, Box<dyn FnMut() -> Element + 'static>>,
    /// Stashed so `tick_callback` can bump it each frame. Once tap event
    /// delivery lands the user's tap handler (set inside
    /// `counter_render_tree`) becomes the canonical mutator and this
    /// goes away.
    tick_signal: Signal<i32>,
}

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

struct InitCtx {
    engine: *mut LyraEngine,
}

#[no_mangle]
pub extern "C" fn lyra_mobile_app_main(engine_raw: *mut c_void) {
    if engine_raw.is_null() {
        return;
    }
    let ctx = Box::new(InitCtx {
        engine: engine_raw as *mut LyraEngine,
    });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    unsafe { lyra_bridge_dispatch(engine_raw as *mut LyraEngine, init_callback, user_data) };
}

extern "C" fn init_callback(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    let ctx: Box<InitCtx> = unsafe { Box::from_raw(user_data as *mut InitCtx) };

    let renderer = match unsafe { BridgeRenderer::from_raw(ctx.engine) } {
        Some(r) => r,
        None => return,
    };

    let count = use_signal(|| 0_i32);
    let app_fn: Box<dyn FnMut() -> Element + 'static> =
        Box::new(move || counter_render_tree(count));

    let runtime = Runtime::new(renderer, app_fn);

    APP_STATE.with(|s| {
        *s.borrow_mut() = Some(AppState {
            runtime,
            tick_signal: count,
        });
    });
}

#[no_mangle]
pub extern "C" fn lyra_mobile_tick(engine_raw: *mut c_void) {
    if engine_raw.is_null() {
        return;
    }
    unsafe {
        lyra_bridge_dispatch(
            engine_raw as *mut LyraEngine,
            tick_callback,
            std::ptr::null_mut(),
        )
    };
}

extern "C" fn tick_callback(_user_data: *mut c_void) {
    APP_STATE.with(|s| {
        let mut s = s.borrow_mut();
        match s.as_mut() {
            Some(state) => {
                state.tick_signal.update(|n| n + 1);
                let n = state.runtime.frame();
                // Use eprintln so it appears in the simulator's stderr.
                eprintln!(
                    "[lyra-mobile] tick: signal -> {}, patches -> {}",
                    state.tick_signal.get(),
                    n,
                );
            }
            None => {
                eprintln!("[lyra-mobile] tick: APP_STATE is None");
            }
        }
    });
}
