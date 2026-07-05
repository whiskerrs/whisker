//! Reusable bootstrap helpers the `#[whisker::main]` macro calls into.
//!
//! User crates don't import this directly. They write:
//!
//! ```ignore
//! use whisker::prelude::*;
//!
//! #[whisker::main]
//! fn app() -> Element {
//!     render! { page { text { "Hello" } } }
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
//! 4. We call `view::set_root(root)` and `view::flush()` to commit
//!    the initial frame.
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
    set_inline_styles, set_root,
};

thread_local! {
    /// `true` between the start of `tick()` and the completion of its
    /// dispatched callback. Used to report idle/busy back to the
    /// host. On our current setup TASM thread == caller thread and
    /// the callback runs synchronously, so this is flipped back to
    /// `false` before `tick()` returns.
    static PENDING: Cell<bool> = const { Cell::new(false) };
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
    // Install stdout/stderr capture BEFORE the user's `app_fn` runs so
    // every `println!` / `log::*` / panic message from user code reaches
    // `whisker run`. No-op in release builds (the hot-reload feature
    // gates the module out entirely).
    start_log_capture();
    // Boxed init context, handed across the C ABI via raw pointer.
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
    let mut ctx: Box<InitCtx> = unsafe { Box::from_raw(user_data as *mut InitCtx) };

    let renderer = match unsafe { BridgeRenderer::from_raw(ctx.engine) } {
        Some(r) => r,
        None => return,
    };

    // Wire host wake-up before we touch any reactive primitive — any
    // signal writes during the initial `app()` run (lazy state
    // initialisers, eager effects) need to schedule a frame correctly.
    whisker_runtime::host_wake::set_request_frame_callback(
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

    // Register the "drive the runtime now" callback. When a background
    // worker marshals a result onto the main thread via
    // `run_on_main_thread` (the `run_blocking` / `resource()` path),
    // the trampoline runs ON the main thread and invokes this — running
    // a full `tick_frame` (flush + drain task pool + flush + mounts +
    // paint) right there on the main-run-loop post. The async
    // completion is drained and rendered immediately, with the vsync
    // render loop untouched. This is the proper fix for the resource
    // hang (replaces the interim busy-tick workaround): no continuous
    // ticking, and no race against a paused CADisplayLink/Choreographer.
    whisker_runtime::main_thread::set_drive_callback(Some(drive));

    // Stand up the tokio runtime (feature-gated) and `enter()` its
    // context on THIS (TASM) thread before any user code runs. We're on
    // the same thread that `tick_frame` later polls the task pool on, so
    // keeping the context entered here means every future poll can find
    // tokio's reactor — making `reqwest` / `spawn_blocking` / `tokio::time`
    // work directly inside `resource()` fetchers. No-op without the feature.
    init_tokio_runtime();

    // Route the platform reporter's events through Whisker's Rust-side
    // propagation reconstruction (capture/bubble/catch over the
    // driver's own element tree). The bridge calls this dispatcher
    // whenever its reporter hook fires.
    super::renderer::register_event_dispatcher();

    // Route CORE-originated custom events (the `<list>` scroll family)
    // through the queue-and-drain channel — they fire from inside
    // Lynx's engine pipeline, so they can't dispatch inline like
    // reporter events. Requires the fork capi tail-added after ABI v2;
    // on an older Lynx this returns false and list events stay dark
    // (exactly the pre-feature behaviour), so no hard failure here.
    if !super::renderer::register_custom_event_dispatcher(ctx.engine) {
        eprintln!(
            "whisker: Lynx build lacks lynx_shell_set_custom_event_callback; \
             <list> scroll/snap/layoutcomplete events will not fire"
        );
    }

    // Install the bridge renderer into the thread-local before
    // running user code. The `render!` macro's `view::*` calls
    // route through whatever is installed here.
    let _prev = install_renderer(Box::new(renderer) as Box<dyn DynRenderer>);

    // Mark main-thread render work in progress for the duration of the
    // initial render. If user code (e.g. a module's startup wiring like
    // whisker-audio's `Player::new`) calls `run_on_main_thread` and the
    // host dispatcher runs the trampoline INLINE on this thread, the
    // trampoline must NOT re-enter `tick_frame` mid-render — the guard
    // makes it defer to a vsync frame instead.
    let _main_work = whisker_runtime::main_thread::MainWorkGuard::new();

    // Run the user's app fn (already-`subsecond::call`-wrapped by
    // the macro when the `hot-reload` feature is on).
    let Some(mut app_fn) = ctx.app_fn.take() else {
        return;
    };
    let Some(app_hash_fn) = ctx.app_hash_fn.take() else {
        return;
    };

    // Run the app under a persistent ROOT OWNER. `#[whisker::main]`'s
    // body runs here, and `provide_context(...)` calls at that bare
    // `app()` level need a current owner to attach to — without one they
    // silently no-op, and any descendant that does
    // `use_context::<T>().expect(...)` then panics across this `extern
    // "C"` boundary (aborting on Android, blank-screening on iOS). The
    // owner is a *detached root*: it lives for the process lifetime and
    // is never disposed. What the app run creates attaches to a child
    // "run owner" instead — that one IS disposable, which is what lets
    // the hot reload full remount tear down one app() run (contexts,
    // signals, effects and all) and start another.
    let root_owner = whisker_runtime::reactive::Owner::detached_root();
    root_owner.with(|| {
        // Whisker owns the root `page`. Lynx requires the shell's root to
        // be a `page`-tagged element and keeps that element FIXED for the
        // app's lifetime (it can't be swapped — see
        // `whisker_bridge_set_root`). So rather than make the user declare
        // it, we create one stable page here with a minimal, layout-only
        // base style (full-size flex column) and mount the app's content
        // as its child. Because the content is attached via
        // `append_child`, a top-level `#[component]` lands with a real
        // parent and hot-reloads through the normal child-remount path;
        // the page itself never moves. Visual styling (background, safe
        // area, padding) belongs on the user's root view, not here.
        let page = create_element(ElementTag::Page);
        set_inline_styles(
            page,
            "display:flex;flex-direction:column;flex-grow:1;flex-shrink:1;",
        );
        // Per-run owner: everything one `app()` invocation creates
        // (top-level contexts included) hangs off this, so a hot reload
        // full remount can dispose it wholesale without touching the
        // root owner or the page.
        let run_owner = whisker_runtime::reactive::Owner::new(None);
        let content = run_owner.with(&mut app_fn);
        append_child(page, content);
        // Commit the initial tree: mark the page as root and ask Lynx
        // to run the first layout+paint pass.
        set_root(page);
        renderer_flush();
        // Fire on_mount callbacks for everything that just mounted. Has
        // to happen after the renderer flush so user-side code that asks
        // "is my view in the tree?" sees it.
        reactive_flush_mounts();
        // Stash what the full-remount path needs. No-op
        // without the hot-reload feature (the closures are dropped).
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

// Build a multi-thread tokio runtime and keep its context entered on the
// TASM thread for the whole process. Must be multi-thread: a
// current-thread runtime only drives its reactor inside `block_on`, but
// Whisker polls futures via `run_until_stalled`, so a current-thread
// reactor would never advance the IO our futures register.
#[cfg(feature = "tokio")]
fn init_tokio_runtime() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all() // IO (mio: epoll on Android / kqueue on iOS) + timer
        .worker_threads(2) // conservative for mobile; tune later if needed
        .thread_name("whisker-tokio")
        .build()
        .expect("whisker: build tokio runtime");
    // Leak the runtime to `'static`, then `forget` the EnterGuard so its
    // Drop never runs — the context stays entered on this thread (which
    // lives for the process lifetime) and tokio's background threads keep
    // driving the reactor regardless of who polls the futures.
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

/// Apply the next pending hot patch, if any. Returns `true` when a
/// patch was successfully applied so the caller can force a flush
/// even if no signal is dirty — Strategy C (A6) will use this to
/// remount affected components. Until then a hot patch only re-binds
/// effect closure bodies; their re-run is up to whatever future
/// scheduling A6 adds.
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

/// Full-remount escalation (still hot reload). Escalates a just-applied patch to a
/// complete `app()` re-run when the per-component remount path can't
/// express it:
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
/// All state is lost by design — this is still sub-second and keeps
/// the process (and the dev-session socket) alive, unlike a full reload
/// reinstall.
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

        // Detach the old content BEFORE disposing its owners.
        // `Owner::dispose` invalidates element handles (renderer slot
        // → `None`), and removing an already-invalidated child
        // silently no-ops against Lynx — the stale subtree would stay
        // on screen. Same ordering rule as the batched path in
        // `remount_components_for`.
        let old_children = whisker_runtime::view::children_of(state.page);
        for child in &old_children {
            whisker_runtime::view::remove_child(state.page, *child);
        }
        state.run_owner.dispose();

        let run_owner = whisker_runtime::reactive::Owner::new(None);
        let content = run_owner.with(|| (state.app_fn)());
        append_child(state.page, content);
        state.run_owner = run_owner;
        // No flush here: the caller is `tick_frame`, whose tail
        // (reactive_flush → flush_mounts → renderer_flush) paints the
        // new tree and fires its on_mount callbacks in this same
        // frame — identical to how per-component remounts land.
    });
}

#[cfg(not(feature = "hot-reload"))]
fn maybe_full_remount(_stats: whisker_runtime::reactive::RemountStats) {}

/// Process one frame on demand. Returns `true` when the runtime is
/// idle after this tick so the host can pause its render loop until the
/// next `request_frame` callback fires.
///
/// Idle is `!dispatch_pending && !has_pending_work()` — the dispatched
/// frame completed AND the reactive queue is genuinely drained. The
/// second term is the **level-triggered** backstop: a native-view
/// layout/measure callback can re-enter Rust during the final
/// `renderer_flush` of `tick_frame` and `schedule()` a signal write.
/// That node lands in `rt.pending` but is past the drain, so the frame
/// finishes with work still queued. Were idle purely `!dispatch_pending`
/// the host would pause its vsync loop here, and because `schedule()`
/// only wakes on the empty→non-empty edge, the leftover (non-empty)
/// queue means no further `set()` ever fires a wake — a permanent wedge.
/// Reporting busy while `has_pending_work()` keeps the host ticking
/// until the queue empties, so a frame always re-runs while work remains.
///
/// An outstanding async task (e.g. a `resource()` fetch parked on a
/// `run_blocking` worker) does NOT keep the host ticking: its completion
/// is resumed off the **main run loop**, not vsync. When the worker
/// marshals its result via `run_on_main_thread`, the trampoline runs on
/// the main thread and invokes the registered drive callback ([`drive`]),
/// which runs a full `tick_frame` right there — draining the pool and
/// painting the result. So the host can safely sleep its vsync loop on
/// idle; the parked fetch resumes via a race-free main-loop post, not a
/// clobberable vsync unpause. (`has_pending_work()` likewise excludes
/// outstanding tasks; see the `cross_thread_wake` resource tests.)
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
    // Evaluate `has_pending_work()` AFTER the dispatched `tick_callback`
    // has run (the bridge dispatch above completes synchronously on the
    // TASM==main thread), so it observes any node a commit-time re-entry
    // left in the queue.
    //
    // The custom-event queue is part of the same level-triggered idle
    // test: a core-originated event (list `layoutcomplete` / `scroll`)
    // queued DURING this tick's own `renderer_flush` already had its
    // `wake_runtime()` edge consumed by the tick in progress, so
    // reporting idle here would pause the vsync loop with the event
    // stranded in the queue.
    !dispatch_pending
        && !whisker_runtime::reactive::has_pending_work()
        && !super::renderer::has_pending_custom_events()
}

extern "C" fn tick_callback(_user_data: *mut c_void) {
    // Contain panics from user code (effects, async tasks, on_mount
    // callbacks) so a single bad `unwrap()` degrades to "this frame is
    // dropped, the app keeps running" instead of unwinding across the C
    // ABI and aborting the whole process. The runtime's internal RAII
    // guards (`flush`'s flushing flag, `run_node_if_alive`'s tracker /
    // owner-stack restore) keep reactive state consistent after a caught
    // panic, so the next tick proceeds cleanly.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(tick_frame));
    if let Err(_panic) = result {
        // The panic message itself already reached stderr via the
        // default hook (captured and forwarded to `whisker run` in dev).
        log_tick_panic();
    }
    // Always clear PENDING so `tick()` reports a definite idle/busy
    // state even when the frame bailed out mid-way.
    PENDING.with(|p| p.set(false));
}

/// "Drive the runtime now" callback, registered with
/// `whisker_runtime::main_thread::set_drive_callback`. Invoked by the
/// `run_on_main_thread` trampoline — which already runs on the Lynx
/// TASM (main) thread — right after a worker marshals its result back.
///
/// Runs the same panic-guarded `tick_frame` as [`tick_callback`]: a
/// full frame (reactive flush + task-pool drain + flush + mounts +
/// renderer paint), so the just-marshaled async completion is drained
/// and rendered immediately on this main-run-loop post. The vsync
/// render loop is untouched.
///
/// Unlike `tick_callback` this does NOT touch `PENDING`: that flag is
/// the bridge-dispatch's idle/busy bookkeeping for the vsync `tick()`
/// path, and this drive is a self-contained main-loop drain, not a
/// host-requested vsync tick. (TASM thread == caller thread, so this
/// runs synchronously here.)
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

    // Drain any pending hot-reload patch before the reactive flush so
    // any patched closures run with their new bodies when the queue
    // fires. Returns the list of host-side fn pointers that were
    // rewritten; empty if no patch was pending or the patch failed.
    let patched = apply_pending_hot_patch();

    if !patched.is_empty() {
        // Per-component remount: dispose + re-mount every
        // `#[component]` whose fn was patched, so structural
        // changes (new elements, new signals) reflect in the
        // visible tree. State local to the remounted component is
        // lost; state held in context / above the remount point
        // survives. Sites whose props layout changed are refused
        // here (re-running their stored closures would be UB) and
        // reported through `stats.layout_changed`.
        let stats = remount_components_for(&patched);
        // Full-remount escalation: escalate to a full `app()` re-run when the
        // per-component path can't express this patch (app() body
        // edited, props layout changed, or no mounted component
        // matched).
        maybe_full_remount(stats);
    }
    // Dispatch core-originated events (`<list>` scroll family) queued
    // since the last frame — before the reactive flush, so handler
    // signal writes render in this same frame.
    super::renderer::drain_custom_events();
    // Advance the continuous animation engine (whisker-animation) by
    // one frame *before* the reactive flush, so any progress signal it
    // writes is drained and painted in this same frame. `step` feeds a
    // monotonic millisecond timestamp; the engine derives each
    // controller's progress from elapsed time, sets its value signal,
    // and reports (via `anim_hook::is_animating`, ORed into
    // `has_pending_work`) whether the host should schedule another
    // frame. No-op when no controller is active — idle costs nothing.
    whisker_runtime::anim_hook::step(monotonic_millis());
    reactive_flush();
    // Drive any async tasks (resource() fetchers, user-spawned
    // futures) until they stall. Tasks that resolve here may write
    // signals; we run another reactive_flush below to surface those
    // writes in the same frame.
    whisker_runtime::tasks::run_until_stalled();
    reactive_flush();
    // Drain on_mount queue *after* the reactive flush — effects that
    // ran this tick may have mounted new components (via `<Show>`
    // flipping true, `<For>` adding an item, etc.), and those
    // newly-mounted components' on_mount callbacks belong to this
    // frame.
    reactive_flush_mounts();
    renderer_flush();
    // A native-view layout/measure callback can re-enter Rust during the
    // commit above and schedule a signal write. Drain that settle in the
    // SAME frame so it neither lags a frame nor (combined with the
    // edge-triggered wake) wedges the loop. Bounded: the level-triggered
    // idle in `tick()` is the backstop if a pathological commit-time
    // feedback loop never settles (it degrades to a visible busy-tick, not
    // a silent freeze).
    const SETTLE_CAP: usize = 16;
    let mut settle = 0;
    while whisker_runtime::reactive::has_pending_work() && settle < SETTLE_CAP {
        settle += 1;
        reactive_flush();
        reactive_flush_mounts();
        renderer_flush();
    }
    // (A core-originated event queued DURING this tick — e.g. a
    // `layoutcomplete` fired by this tick's own renderer_flush — is kept
    // alive by `tick()`'s level-triggered idle test, which reports busy
    // while the custom-event queue is non-empty.)
}

/// Monotonic wall-clock time in milliseconds, measured from a fixed
/// process-start anchor. Feeds the animation engine's per-frame
/// `step` so progress advances by real elapsed time. Uses `Instant`
/// (monotonic, never goes backwards) rather than `SystemTime` so a
/// clock adjustment can't jump or stall an animation.
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
