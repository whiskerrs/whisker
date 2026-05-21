//! Backend driver for Whisker.
//!
//! Today there is one backend ([`lynx`]) that talks to the C++ Lynx
//! bridge shipped under `whisker-driver-sys/bridge/`. Future backends
//! (web, wgpu, …) would
//! land as sibling modules behind cfg gates. Users never touch this
//! crate directly — `#[whisker::main]` re-exports the `run` / `tick`
//! helpers from here as `whisker::__main_runtime::{run,tick}`.

pub mod lynx;

pub use lynx::bootstrap;
pub use lynx::renderer::BridgeRenderer;

/// ASLR anchor symbol used by Whisker's vendored subsecond fork to
/// compute the slide between this dylib's cached static layout
/// (recorded by the dev-server at link time) and its runtime load
/// address.
///
/// `dlsym(RTLD_DEFAULT, "whisker_aslr_anchor")` resolves to this
/// fn's runtime address; pairing that with the static address the
/// dev-server captured produces a precise per-dylib base offset.
/// The fn body never executes — only the symbol's address matters.
///
/// ## Why a unique name (not `main`)
///
/// Upstream subsecond anchors on `main`. That works for binaries
/// where `main` is the PIE entry and unique in the linker
/// namespace. Whisker ships the user crate as a `dylib` loaded via
/// Android `System.loadLibrary`, where the namespace can contain
/// several `main` symbols (`app_process64`'s + stale patch memfds')
/// and `dlsym(RTLD_DEFAULT, "main")` picks the wrong one. A
/// Whisker-specific symbol exists only in user dylibs + patches,
/// so the lookup is collision-free regardless of namespace order.
///
/// ## Why it lives here (not in `#[whisker::main]`)
///
/// Every Whisker dylib statically links `whisker-driver`, so the
/// symbol auto-exports from any `whisker` user crate without the
/// `#[whisker::main]` macro needing to inject it. Patch dylibs
/// produced by the hot-reload pipeline also pick up this symbol
/// via `--require-defined whisker_aslr_anchor` (see
/// `whisker-dev-server::hotpatch::patcher`).
///
/// Same `extern "C"` + `#[no_mangle]` shape every Whisker dylib
/// needs; centralising it here means the macro stays focused on
/// turning a user `fn app()` into a `whisker_app_main` FFI entry.
#[no_mangle]
pub extern "C" fn whisker_aslr_anchor() -> std::ffi::c_int {
    0
}
