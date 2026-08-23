//! Reusable bootstrap helpers the `#[whisker::main]` macro calls into.
//!
//! User crates don't import this directly. They write:
//!
//! ```ignore
//! use whisker::prelude::*;
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     render! { view { text(value: "Hello") } }
//! }
//! ```
//!
//! and the macro expands to FFI exports that call [`run`] / [`tick`].
//!
//! ## What happens on mount
//!
//! 1. The C++ bridge dispatches us onto the Lynx TASM thread.
//! 2. We build a `BridgeRenderer` and install it as the thread-local
//!    `DynRenderer` so `view::create_element` / `set_attribute` / …
//!    inside the user's `render!` macro route through the bridge.
//! 3. We invoke `app()`. The user's body runs `render!`, which
//!    populates the Lynx element tree and returns an `Element`
//!    for the root.
//! 4. We attach that root beneath the Lynx-required shell `page`, then
//!    call `view::set_root(page)` and `view::flush()` to commit the frame.
//!
//! ## What happens on tick
//!
//! `tick()` is the host's "you asked us to wake you up" callback. We
//! drain the reactive `flush` queue — running effects whose
//! dependencies have changed since the last tick — then `flush()`
//! the renderer so any element-tree mutations the effects emitted
//! reach the screen. Returns `true` when nothing was pending (the
//! host can park the render loop again).
//!
//! ## Subsecond hot reload
//!
//! On every tick we first try `apply_pending_hot_patch`. If a patch
//! landed, `remount_components_for` disposes and re-mounts every
//! `#[component]` whose fn pointer was rewritten, and
//! `maybe_full_remount` escalates to a complete `app()`
//! re-run when the per-component path can't express the change —
//! the `app()` body itself was edited (detected via the source hash
//! the `#[whisker::main]` macro bakes in), or the patch matched no
//! mounted component at all.

use super::renderer::BridgeRenderer;
use std::cell::Cell;
use std::ffi::c_void;

use whisker_driver_sys::{WhiskerEngine, whisker_bridge_dispatch};
use whisker_runtime::element::ElementTag;
use whisker_runtime::reactive::{
    flush as reactive_flush, flush_mounts as reactive_flush_mounts, remount_components_for,
};
use whisker_runtime::view::{
    DynRenderer, Element, append_child, create_element, flush as renderer_flush, install_renderer,
    set_inline_styles, set_root, uninstall_renderer,
};

thread_local! {
    /// `true` between the start of `tick()` and the completion of its
    /// dispatched callback. Used to report idle/busy back to the
    /// host. On our current setup TASM thread == caller thread and
    /// the callback runs synchronously, so this is flipped back to
    /// `false` before `tick()` returns.
    static PENDING: Cell<bool> = const { Cell::new(false) };

    /// The current bootstrap's app-root owner. Retained so a
    /// re-`run()` in the same process — Android recreates the Activity
    /// (hence the WhiskerView, the engine, and this bootstrap) after a
    /// back-out finishes it while the process lives on — can tear the
    /// previous app tree down before building the next one. Without
    /// this the second tree stacks on top of the first in the shared
    /// thread-local runtime and never paints (issue #396).
    static APP_ROOT_OWNER: std::cell::RefCell<Option<whisker_runtime::reactive::Owner>> =
        const { std::cell::RefCell::new(None) };
}

/// Bootstrap the runtime. Called from the FFI export the
/// `#[whisker::main]` macro generates. Users do not call this
/// directly.
///
/// `request_frame` is the host's "wake up the render loop" callback;
/// signal updates fire it so the host can unpause its `CADisplayLink`
/// (or equivalent) to schedule the next tick. May be `None` if the
/// host runs an unconditional render loop.
pub fn run<F, H>(
    engine_raw: *mut c_void,
    request_frame: Option<extern "C" fn(*mut c_void)>,
    request_frame_data: *mut c_void,
    app_fn: F,
    app_hash_fn: H,
) where
    F: FnMut() -> Element + 'static,
    H: Fn() -> u64 + 'static,
{
    if engine_raw.is_null() {
        return;
    }
    // Must precede the user's `app_fn` so every `println!` / `log::*` /
    // panic message from user code reaches `whisker run`. No-op without
    // the hot-reload feature.
    start_log_capture();
    let ctx = Box::new(InitCtx {
        engine: engine_raw as *mut WhiskerEngine,
        app_fn: Some(Box::new(app_fn) as Box<dyn FnMut() -> Element + 'static>),
        app_hash_fn: Some(Box::new(app_hash_fn) as Box<dyn Fn() -> u64 + 'static>),
        request_frame,
        request_frame_data,
    });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    unsafe { whisker_bridge_dispatch(engine_raw as *mut WhiskerEngine, init_callback, user_data) };
}

struct InitCtx {
    engine: *mut WhiskerEngine,
    /// `Option` because we move the closure out inside `init_callback`
    /// to call it. `FnMut` (not `FnOnce`): the initial mount invokes it
    /// once, and the hot-reload full-remount path keeps it around to re-run
    /// `app()` from scratch when a patch changes code no `#[component]`
    /// remount can reflect (the `app()` body itself). Release builds
    /// still call it exactly once.
    app_fn: Option<Box<dyn FnMut() -> Element + 'static>>,
    /// Reads the app fn's compile-time source hash *through subsecond
    /// dispatch* (see the `#[whisker::main]` macro), so after a patch
    /// it reports the patch dylib's value. The full-remount trigger.
    app_hash_fn: Option<Box<dyn Fn() -> u64 + 'static>>,
    request_frame: Option<extern "C" fn(*mut c_void)>,
    request_frame_data: *mut c_void,
}

extern "C" fn init_callback(user_data: *mut c_void) {
    if user_data.is_null() {
        return;
    }
    // Re-`run()` in a live process (Android Activity recreation): tear
    // the previous app tree down before building the next. Disposing
    // the app-root owner cascades the run owner and every element /
    // signal / on_cleanup below it — including the `BackGuard`s and
    // other registrations tree #1 held. Process-global module
    // singletons (safe-area insets, the back registry, …) are minted
    // under their OWN detached roots, so they survive untouched.
    //
    // Clear every process-global wiring that pointed at engine #1
    // FIRST. The old WhiskerView's `destroy()` called
    // `whisker_bridge_engine_release`, which `delete`s the engine, so
    // the renderer, the host-wake callback, and the main-thread
    // dispatcher all dangle. Any call through them during the dispose
    // (element releases; an `on_cleanup` writing a signal, which wakes
    // the host; a background async completion marshalling onto the main
    // thread) would be a use-after-free. Nulled, they no-op; engine #2's
    // wirings install further down.
    APP_ROOT_OWNER.with(|slot| {
        if let Some(prev) = slot.borrow_mut().take() {
            uninstall_renderer(None);
            whisker_runtime::runtime_wake::set_request_frame_callback(None, std::ptr::null_mut());
            whisker_runtime::main_thread::set_main_thread_dispatcher(None, std::ptr::null_mut());
            whisker_runtime::main_thread::set_drive_callback(None);
            prev.dispose();
        }
    });
    let mut ctx: Box<InitCtx> = unsafe { Box::from_raw(user_data as *mut InitCtx) };

    let renderer = match unsafe { BridgeRenderer::from_raw(ctx.engine) } {
        Some(r) => r,
        None => return,
    };

    // Wire host wake-up before we touch any reactive primitive — any
    // signal writes during the initial `app()` run (lazy state
    // initialisers, eager effects) need to schedule a frame correctly.
    whisker_runtime::runtime_wake::set_request_frame_callback(
        ctx.request_frame,
        ctx.request_frame_data,
    );

    // Wire the main-thread dispatcher so background threads can call
    // `run_on_main_thread(|| { ... })` to marshal work onto the TASM
    // thread. The shim erases the `WhiskerEngine*` to `*mut c_void`
    // because `whisker-runtime` doesn't depend on `whisker-driver-sys`.
    whisker_runtime::main_thread::set_main_thread_dispatcher(
        Some(dispatch_shim),
        ctx.engine as *mut c_void,
    );

    // Lets a background worker that marshals a result back via
    // `run_on_main_thread` run a full `tick_frame` on that main-loop
    // post, so an async completion paints without continuous ticking
    // and without racing a paused CADisplayLink / Choreographer.
    whisker_runtime::main_thread::set_drive_callback(Some(drive));

    // The tokio context must be entered on THIS (TASM) thread, the same
    // one `tick_frame` later polls the task pool on — that is what lets
    // a future poll find tokio's reactor, so `reqwest` / `spawn_blocking`
    // / `tokio::time` work inside `resource()`. No-op without the feature.
    init_tokio_runtime();

    // Route the platform reporter's events through Whisker's Rust-side
    // propagation reconstruction (capture/bubble/catch over the
    // driver's own element tree). The bridge calls this dispatcher
    // whenever its reporter hook fires.
    super::renderer::register_event_dispatcher();

    // CORE-originated custom events (the `<list>` scroll family) fire
    // from inside Lynx's engine pipeline, so they queue-and-drain rather
    // than dispatching inline like reporter events. Requires the fork
    // capi tail-added after ABI v2; an older Lynx just leaves them dark.
    if !super::renderer::register_custom_event_dispatcher(ctx.engine) {
        eprintln!(
            "whisker: Lynx build lacks lynx_shell_set_custom_event_callback; \
             <list> scroll/snap/layoutcomplete events will not fire"
        );
    }

    // The `render!` macro's `view::*` calls route through whatever is
    // installed here, so this must precede any user code.
    let _prev = install_renderer(Box::new(renderer) as Box<dyn DynRenderer>);

    // Mark main-thread render work in progress for the duration of the
    // initial render. If user code (e.g. a module's startup wiring like
    // whisker-audio's `Player::new`) calls `run_on_main_thread` and the
    // host dispatcher runs the trampoline INLINE on this thread, the
    // trampoline must NOT re-enter `tick_frame` mid-render — the guard
    // makes it defer to a vsync frame instead.
    let _main_work = whisker_runtime::main_thread::MainWorkGuard::new();

    let Some(mut app_fn) = ctx.app_fn.take() else {
        return;
    };
    let Some(app_hash_fn) = ctx.app_hash_fn.take() else {
        return;
    };

    // `provide_context(...)` at the bare `app()` level needs a current
    // owner to attach to; without one it silently no-ops and any
    // descendant's `use_context::<T>().expect(...)` panics across this
    // `extern "C"` boundary (aborting on Android, blank-screening on
    // iOS). Disposed only by a re-`run()` in the same process (see the
    // teardown at the top of this fn); a full remount tears down the
    // disposable child "run owner" below instead.
    let root_owner = whisker_runtime::reactive::Owner::detached_root();
    APP_ROOT_OWNER.with(|slot| *slot.borrow_mut() = Some(root_owner));
    root_owner.with(|| {
        // Lynx requires the shell root to be a `page` element and keeps
        // it FIXED for the app's lifetime (it can't be swapped — see
        // `whisker_bridge_set_root`), so whisker owns one here and mounts
        // the app's content as its child. Attaching via `append_child`
        // gives a top-level `#[component]` a real parent, so it
        // hot-reloads through the normal child-remount path. Style stays
        // layout-only; visual styling belongs on the user's root view.
        let page = create_element(ElementTag::Page);
        set_inline_styles(
            page,
            "display:flex;flex-direction:column;flex-grow:1;flex-shrink:1;",
        );
        // Everything one `app()` invocation creates (top-level contexts
        // included) hangs off this, so a full remount disposes it
        // wholesale without touching the root owner or the page.
        let run_owner = whisker_runtime::reactive::Owner::new(None);
        let content = run_owner.with(&mut app_fn);
        append_child(page, content);
        set_root(page);
        renderer_flush();
        // After the renderer flush, so user code asking "is my view in
        // the tree?" from an on_mount sees it.
        reactive_flush_mounts();
        store_hot_app_state(app_fn, app_hash_fn, page, run_owner);
    });

    start_hot_reload_receiver();
}

#[cfg(feature = "hot-reload")]
fn start_hot_reload_receiver() {
    whisker_dev_runtime::start_receiver();
}

#[cfg(not(feature = "hot-reload"))]
fn start_hot_reload_receiver() {}

// Must be multi-thread: a current-thread runtime only drives its reactor
// inside `block_on`, and whisker polls futures via `run_until_stalled`,
// so a current-thread reactor would never advance registered IO.
#[cfg(feature = "tokio")]
fn init_tokio_runtime() {
    // Once per process: the runtime + its entered context outlive any
    // single bootstrap, so a re-`run()` (Android Activity recreation)
    // must not build and leak a second one.
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all() // IO (mio: epoll on Android / kqueue on iOS) + timer
        .worker_threads(2) // conservative for mobile; tune later if needed
        .thread_name("whisker-tokio")
        .build()
        .expect("whisker: build tokio runtime");
    // Leak the runtime and `forget` the EnterGuard so its Drop never
    // runs: the context stays entered on this process-lifetime thread and
    // tokio's background threads keep driving the reactor.
    let rt: &'static tokio::runtime::Runtime = Box::leak(Box::new(rt));
    std::mem::forget(rt.enter());
}

#[cfg(not(feature = "tokio"))]
fn init_tokio_runtime() {}

#[cfg(feature = "hot-reload")]
fn start_log_capture() {
    whisker_dev_runtime::start_log_capture();
}

#[cfg(not(feature = "hot-reload"))]
fn start_log_capture() {}

/// Apply the next pending hot patch, if any, returning the host-side
/// fn pointers it rewrote (empty when nothing was pending or the patch
/// failed). The caller remounts the components those pointers back.
#[cfg(feature = "hot-reload")]
fn apply_pending_hot_patch() -> Vec<*const ()> {
    let Some(table) = whisker_dev_runtime::take_pending_patch() else {
        return Vec::new();
    };
    let entries = table.map.len();
    let lib = table.lib.clone();
    whisker_dev_runtime::devlog(&format!(
        "apply_patch: start (lib={}, entries={entries})",
        lib.display(),
    ));
    let started = std::time::Instant::now();
    // SAFETY: tick_callback runs on the Lynx TASM thread and we call
    // this *before* invoking any user code that might call
    // `subsecond::call`. The only safe window to swap dispatchers.
    match unsafe { subsecond::apply_patch(table) } {
        Ok(patched) => {
            whisker_dev_runtime::devlog(&format!(
                "patch applied ({entries} entries in {:?}, {} fn pointers)",
                started.elapsed(),
                patched.len(),
            ));
            patched
        }
        Err(e) => {
            whisker_dev_runtime::devlog(&format!(
                "apply_patch failed: {e:?} (lib was {})",
                lib.display(),
            ));
            Vec::new()
        }
    }
}

#[cfg(not(feature = "hot-reload"))]
fn apply_pending_hot_patch() -> Vec<*const ()> {
    Vec::new()
}

/// Everything the full-remount path needs to re-run `app()`
/// from scratch. Lives in a TASM-thread-local because both writers
/// (`init_callback`, `maybe_full_remount`) run on that thread only.
#[cfg(feature = "hot-reload")]
struct HotAppState {
    app_fn: Box<dyn FnMut() -> Element + 'static>,
    app_hash_fn: Box<dyn Fn() -> u64 + 'static>,
    /// The stable root page — never recreated (Lynx keeps the shell
    /// root fixed); full remount swaps its children only.
    page: Element,
    /// Owner of the current `app()` run. Disposed and replaced on
    /// full remount, cascading cleanup through every context /
    /// signal / component owner the run created.
    run_owner: whisker_runtime::reactive::Owner,
    /// App-body source hash as of the last (re)run, read through
    /// subsecond dispatch. Compared after each patch.
    last_hash: u64,
}

#[cfg(feature = "hot-reload")]
thread_local! {
    static HOT_APP: std::cell::RefCell<Option<HotAppState>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(feature = "hot-reload")]
fn store_hot_app_state(
    app_fn: Box<dyn FnMut() -> Element + 'static>,
    app_hash_fn: Box<dyn Fn() -> u64 + 'static>,
    page: Element,
    run_owner: whisker_runtime::reactive::Owner,
) {
    let last_hash = app_hash_fn();
    HOT_APP.with(|slot| {
        *slot.borrow_mut() = Some(HotAppState {
            app_fn,
            app_hash_fn,
            page,
            run_owner,
            last_hash,
        });
    });
}

#[cfg(not(feature = "hot-reload"))]
fn store_hot_app_state(
    _app_fn: Box<dyn FnMut() -> Element + 'static>,
    _app_hash_fn: Box<dyn Fn() -> u64 + 'static>,
    _page: Element,
    _run_owner: whisker_runtime::reactive::Owner,
) {
}

/// Escalate a just-applied patch to a complete `app()` re-run when the
/// per-component remount path can't express it:
///
/// - **`app()` body changed** — its source hash (read through
///   subsecond dispatch, so this sees the patch's value) differs
///   from the last run's. `#[component]` remounts re-run component
///   bodies, never `app()` itself, so top-level wiring edits
///   (`provide_context` values, which root component is mounted,
///   page-level layout) would otherwise apply-but-not-render.
/// - **Props layout changed** — `remount_components_for` refused one
///   or more sites because their stored body closures were built
///   against a different props signature than the patched code
///   expects. Only a from-scratch rebuild (fresh `app()` run, all
///   props constructed by patched code) is safe.
/// - **Nothing remounted** — the patch matched no attached mount
///   site. Happens when the app has no top-level `#[component]`
///   (everything inline in `app()`) or when prior teardown left only
///   orphaned sites; without escalation the patch is applied but
///   invisible.
///
/// All state is lost by design; the process (and the dev-session
/// socket) survives, which is what keeps this sub-second.
#[cfg(feature = "hot-reload")]
fn maybe_full_remount(stats: whisker_runtime::reactive::RemountStats) {
    HOT_APP.with(|slot| {
        let mut guard = slot.borrow_mut();
        let Some(state) = guard.as_mut() else {
            return;
        };
        let new_hash = (state.app_hash_fn)();
        let app_changed = new_hash != state.last_hash;
        let reason = if app_changed {
            "app() body changed"
        } else if stats.layout_changed > 0 {
            "props layout changed"
        } else if stats.remounted == 0 {
            "patch matched no mounted component"
        } else {
            return;
        };
        whisker_dev_runtime::devlog(&format!("full remount ({reason})"));
        state.last_hash = new_hash;

        // Detach the old content BEFORE disposing its owners:
        // `Owner::dispose` invalidates element handles, and removing an
        // already-invalidated child silently no-ops against Lynx, so the
        // stale subtree would stay on screen.
        let old_children = whisker_runtime::view::children_of(state.page);
        for child in &old_children {
            whisker_runtime::view::remove_child(state.page, *child);
        }
        state.run_owner.dispose();

        let run_owner = whisker_runtime::reactive::Owner::new(None);
        let content = run_owner.with(|| (state.app_fn)());
        append_child(state.page, content);
        state.run_owner = run_owner;
        // No flush here: the caller is `tick_frame`, whose tail paints
        // the new tree and fires its on_mount callbacks this frame.
    });
}

#[cfg(not(feature = "hot-reload"))]
fn maybe_full_remount(_stats: whisker_runtime::reactive::RemountStats) {}

/// Process one frame on demand. Returns `true` when the runtime is
/// idle after this tick so the host can pause its render loop until the
/// next `request_frame` callback fires.
///
/// Idle is `!dispatch_pending && !has_pending_work()`. The second term
/// is the **level-triggered** backstop: a native-view layout/measure
/// callback can re-enter Rust during `tick_frame`'s final
/// `renderer_flush` and `schedule()` a write that lands past the drain.
/// Since `schedule()` only wakes on the empty→non-empty edge, pausing
/// the vsync loop with that queue non-empty means no later `set()` ever
/// wakes it again — a permanent wedge. Reporting busy while
/// `has_pending_work()` keeps a frame re-running until it empties.
///
/// An outstanding async task does NOT keep the host ticking, and
/// `has_pending_work()` excludes them: a `resource()` fetch parked on a
/// `run_blocking` worker resumes off the **main run loop** via
/// [`drive`], not vsync, so sleeping the vsync loop is safe.
pub fn tick(engine_raw: *mut c_void) -> bool {
    if engine_raw.is_null() {
        return true;
    }
    PENDING.with(|p| p.set(true));
    unsafe {
        whisker_bridge_dispatch(
            engine_raw as *mut WhiskerEngine,
            tick_callback,
            std::ptr::null_mut(),
        )
    };
    let dispatch_pending = PENDING.with(|p| p.get());
    // Evaluated AFTER the dispatched `tick_callback` (the bridge
    // dispatch completes synchronously on the TASM==main thread) so it
    // observes anything a commit-time re-entry left queued. The
    // custom-event queue joins the same test: an event queued during
    // this tick's own `renderer_flush` already had its `wake_runtime()`
    // edge consumed, so reporting idle would strand it.
    !dispatch_pending
        && !whisker_runtime::reactive::has_pending_work()
        && !super::renderer::has_pending_custom_events()
}

extern "C" fn tick_callback(_user_data: *mut c_void) {
    // Contain user-code panics so a bad `unwrap()` drops one frame
    // instead of unwinding across the C ABI and aborting the process.
    // The runtime's RAII guards leave reactive state consistent after a
    // caught panic, so the next tick proceeds cleanly.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(tick_frame));
    if let Err(_panic) = result {
        log_tick_panic();
    }
    // Cleared unconditionally so `tick()` reports a definite idle/busy
    // state even when the frame bailed out mid-way.
    PENDING.with(|p| p.set(false));
}

/// "Drive the runtime now" callback, registered with
/// `whisker_runtime::main_thread::set_drive_callback`. Invoked by the
/// `run_on_main_thread` trampoline — which already runs on the Lynx
/// TASM (main) thread — right after a worker marshals its result back.
///
/// Runs the same panic-guarded `tick_frame` as [`tick_callback`], so
/// the just-marshaled async completion paints on this main-run-loop
/// post without touching the vsync loop.
///
/// Deliberately does NOT touch `PENDING`: that flag is the bridge
/// dispatch's idle/busy bookkeeping for the vsync `tick()` path, and
/// this is a self-contained main-loop drain.
extern "C" fn drive() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(tick_frame));
    if result.is_err() {
        log_tick_panic();
    }
}

/// The body of one frame. Split out of `tick_callback` so the whole
/// thing can run under `catch_unwind` without an `extern "C"` closure.
fn tick_frame() {
    // Mark render/tick work in progress so a re-entrant
    // `run_on_main_thread` dispatch (some hosts run same-thread posts
    // inline) defers to a vsync frame instead of re-entering this body.
    let _main_work = whisker_runtime::main_thread::MainWorkGuard::new();

    // Before the reactive flush, so patched closures run with their new
    // bodies when the queue fires.
    let patched = apply_pending_hot_patch();

    if !patched.is_empty() {
        // Sites whose props layout changed are refused here —
        // re-running their stored closures would be UB — and reported
        // through `stats.layout_changed`.
        let stats = remount_components_for(&patched);
        maybe_full_remount(stats);
    }
    // Both of these run before the reactive flush so the signal writes
    // they produce are drained and painted in this same frame.
    super::renderer::drain_custom_events();
    whisker_runtime::anim_hook::step(monotonic_millis());
    reactive_flush();
    // Tasks that resolve here may write signals, hence the second flush.
    whisker_runtime::tasks::run_until_stalled();
    reactive_flush();
    // After the reactive flush: effects that ran this tick may have
    // mounted new components, and their on_mount callbacks belong to
    // this frame.
    reactive_flush_mounts();
    renderer_flush();
    // A native-view layout/measure callback can re-enter Rust during the
    // commit above and schedule a write; draining it in the SAME frame
    // keeps it from lagging or, with the edge-triggered wake, wedging the
    // loop. Capped because `tick()`'s level-triggered idle is the
    // backstop for a commit-time feedback loop that never settles.
    const SETTLE_CAP: usize = 16;
    let mut settle = 0;
    while whisker_runtime::reactive::has_pending_work() && settle < SETTLE_CAP {
        settle += 1;
        reactive_flush();
        reactive_flush_mounts();
        renderer_flush();
    }
}

/// Milliseconds since a fixed process-start anchor, feeding the
/// animation engine's per-frame `step`. `Instant` rather than
/// `SystemTime` so a clock adjustment can't jump or stall an
/// animation.
fn monotonic_millis() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static ANCHOR: OnceLock<Instant> = OnceLock::new();
    let anchor = ANCHOR.get_or_init(Instant::now);
    anchor.elapsed().as_secs_f64() * 1000.0
}

#[cfg(feature = "hot-reload")]
fn log_tick_panic() {
    whisker_dev_runtime::devlog("tick: user code panicked; frame dropped, app continues");
}

#[cfg(not(feature = "hot-reload"))]
fn log_tick_panic() {
    eprintln!("whisker: panic in tick; frame dropped, app continues");
}

/// Type-erased shim handed to `whisker_runtime::main_thread`. The
/// runtime crate stores the engine as `*mut c_void` (it doesn't
/// depend on `whisker-driver-sys`); we cast back here before
/// invoking the C bridge.
extern "C" fn dispatch_shim(
    engine: *mut c_void,
    callback: extern "C" fn(*mut c_void),
    user_data: *mut c_void,
) -> bool {
    if engine.is_null() {
        return false;
    }
    unsafe { whisker_bridge_dispatch(engine as *mut WhiskerEngine, callback, user_data) }
}
