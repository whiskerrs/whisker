# Hot-reload internals

How Whisker's dev loop reflects source edits onto a running app, as it
works today. Audience: contributors hacking on `whisker-dev-server`,
`whisker-dev-runtime`, `whisker-driver`, or the vendored
`whisker-subsecond`.

## Hot Reload and Full Reload

`whisker run` watches the user crate's `src/` (plus every workspace
path-dep `src/`). There are two reload mechanisms, with one hard rule:
**saves only ever hot-reload; a Full Reload runs only when the user
presses `R`** (formerly "Tier 1" / "Tier 2" with automatic fallback —
the fallback restarted the app mid-interaction, which cost more than
it saved).

- **Full Reload** (`R`). Full `cargo build` → re-install
  (`adb install` / `simctl install`) → relaunch. 5–30 s. Always
  correct, never preserves state. The only path that picks up
  dependency-graph changes.
- **Hot Reload** (on save, or `r` to force a patch without a change).
  Rebuild *only the changed crate* into a thin patch dylib, ship it
  over a WebSocket, and swap function pointers on-device via
  `subsecond::apply_patch`. Sub-second on a warm cache. Signals,
  scroll positions, and animation phase survive because we replace
  function bodies, not the whole binary.

The per-change decision is `decide_action(kind, has_patcher)` in
`whisker-dev-server/src/lib.rs`:

| `ChangeKind` | Action |
|---|---|
| `Other` (assets, README, …) | `Ignore` |
| `CargoToml` (also `Cargo.lock`) | `PromptFullReload` — the dep graph may have moved; a patch can't reload deps |
| `RustCode` *and* a `Patcher` is available | `HotReload` |
| `RustCode`, no `Patcher` | `PromptFullReload` |

`PromptFullReload` **does not rebuild**: it prints the reason, emits
`Event::FullReloadRequired` (the TUI keeps a persistent "press R"
banner up until a Full Reload starts), and waits. Explicit commands
arrive through `DevCommand::{HotReload, FullReload}` on the channel
the CLI wires to the `r` / `R` keys.

Change classification lives in `watcher.rs` (`Change::classify` picks
the most disruptive kind among a batch). `whisker run` defaults to
`HotPatchMode::HotReload`; `--no-hot-patch` forces `FullReloadOnly`
(every save prompts).

Hot Reload never falls back to a rebuild on its own. Failure handling:

- **Infrastructure failures prompt.** Patcher init failed, no client
  `aslr_reference` yet, a multi-crate change batch, the link failed,
  the dylib couldn't be read — each reports its reason and prompts
  for `R`. The dev loop is never killed by a transient patch glitch.
- **Compile errors just wait.** When the thin rebuild's rustc exits 1
  (`RustcRejectedCode` in `hotpatch/runner.rs`), the user's code
  doesn't compile — a Full Reload would fail with the same
  diagnostics after a much longer wait. The loop reports
  `compile error — fix the code and save again` and stays put.
- **Patcher init failure isn't permanent.** If `Patcher::initialize`
  fails at startup, the loop retries it on every subsequent
  `RustCode` change (Full Reloads run with the capture shims wired,
  so the caches it reads may have been repopulated since).

## Hot Reload pipeline, end to end

### 1. The fat build doubles as a capture pass

When `hot_patch_mode == HotReload`, the *initial* build (the one
that produces the installable artifact) runs with the capture shims
wired in (`prepare_hot_reload_capture` in `lib.rs`):

- `RUSTC_WORKSPACE_WRAPPER` = `whisker-rustc-shim` — records each
  rustc invocation's argv to `<workspace>/target/.whisker/cache/rustc-args/`,
  along with the `CARGO_*` / `OUT_DIR` environment cargo set for it
  (minus `CARGO_MAKEFLAGS`, whose jobserver fds die with cargo). The
  thin rebuild replays those vars so `env!("CARGO_PKG_*")` /
  `env!("OUT_DIR")` in user code still compiles under the raw
  (cargo-less) rustc spawn.
- `-C linker=whisker-linker-shim` — records each linker invocation's
  argv (keyed by output basename) to the linker-args cache, then
  forwards to the real linker.

After that build completes the caches are populated, and
`Patcher::initialize` (`hotpatch/patcher.rs`) reads them plus the
original on-device binary's symbol table (`HotpatchModuleCache`).
Initialization is non-fatal: failure logs a warning, saves prompt for
Full Reloads, and the init is retried on later changes.

The shims are `[[bin]]` targets of the `whisker-cli` package, resolved
by `hotpatch/shim_paths.rs::resolve_shim_paths` in this order:

1. **Beside the running `whisker` binary** (`current_exe()`'s dir).
   `cargo install whisker-cli` drops all three bins into `~/.cargo/bin`
   together, so crates.io users resolve here with no workspace build.
2. **`<target>/debug/`** (`CARGO_TARGET_DIR` if set, else
   `<workspace>/target`).
3. **Build them** — `cargo build -p whisker-cli --bin whisker-rustc-shim
   --bin whisker-linker-shim`, then re-check. Only meaningful in-workspace.

### 2. Thin rebuild → patch dylib

On a `RustCode` change, `Patcher::build_patch(aslr_reference, crate_key)`
runs:

1. **`rustc --emit=obj`** for the changed crate, replaying its captured
   rustc args (`thin_build::build_obj_plan` + `run_obj_plan`). This
   yields an `.o` that still contains every `pub fn`'s mangled symbol —
   object files are pre-link, so dead-code elimination and cdylib
   symbol stripping haven't run yet. (Going through cargo's `cdylib`
   path here would silently drop every mangled `pub fn`; that's why the
   pipeline drives `rustc`/the linker directly.)
2. **Stub object synthesis** (`stub_object`, Option B / Dioxus-style).
   For each undefined symbol the patch references, a tiny ARM64/Mach-O
   stub defines a trampoline to that symbol's *live runtime address* in
   the host, computed from the host's static address plus the ASLR
   slide (`aslr_reference - cache.aslr_reference`). The stub bytes are
   cached per-session keyed by the FNV-1a hash of the needed-symbol set,
   so body-only edits reuse them. On Linux/Android the host `.so` is
   also added to the link line as a `DT_NEEDED` fallback for the
   non-Text symbols the weak stubs don't cover (thread-locals, statics).
3. **Explicit link** (`build_link_plan` + `run_link_plan`) of the thin
   `.o` (+ stub + extras) into a patch dylib, replaying the captured
   linker args. On macOS, `_whisker_aslr_anchor` / `_whisker_app_main` /
   `_whisker_tick` are force-exported so subsecond's `dlsym(patch, …)`
   resolves.
4. **JumpTable construction** (`symbol_table` + `cache` +
   `build_jump_table`). Parse the patch dylib's symbols with the
   `object` crate, diff against the cached original, and emit a
   `subsecond_types::JumpTable` of `old_addr → new_addr` pairs.

Sub-crate patches (`crate_key != user crate`) additionally rebuild the
*user* crate's `.o` and link it in, because the user crate carries the
`whisker_aslr_anchor` symbol subsecond needs in the patch dylib's
`.dynsym`.

### 3. Wire format and device-side handshake

The patch is broadcast over the WebSocket at `ws://<bind>/whisker-dev`.
Connection direction is **device → host**: the on-device
`whisker-dev-runtime` dials the host's `whisker run`.

Patches are **binary** frames:

```text
[8 bytes: u64 BE — JSON header length]
[N bytes:        JSON header { "kind": "patch", "table": {...} }]
[rest:           raw patch dylib bytes (no encoding) ]
```

No base64 — the dylib ships verbatim (~tens of KB). The JumpTable's
`map` is serialized as an array of `[old, new]` pairs (JSON object keys
can only be strings, which round-trips badly through the custom
`u64`-keyed `AddressMap`); both sides share that adapter
(`server::wire_jump_table` ↔ `hot_reload::deserialize_jump_table`).

On connect, the device sends a **text** `hello` frame:

```json
{ "kind": "hello", "aslr_reference": <u64>, "token": "<hex>" }
```

The `aslr_reference` is the device's `subsecond::aslr_reference()` —
the runtime address of the `whisker_aslr_anchor` symbol. The server
stashes it (single-slot, last-write-wins) so the patcher can compute
the ASLR slide and bake host runtime addresses into the stub objects.

**Why the handshake is needed.** The stub-asm approach (Option B)
resolves every host symbol the patch references at *build* time, baking
absolute runtime addresses into trampolines, rather than relying on
`dlopen`-time symbol resolution against the host. That only works if the
host knows the device's live load base — which is exactly
`aslr_reference`. Until a client reports its `aslr_reference`,
`lib.rs::run` withholds Hot Reload and prompts for a Full Reload. The value is
cleared on disconnect, because reusing a dead process's slide for the
next process would stamp trampolines against meaningless addresses and
crash the device.

### 4. Device materialises and applies

`whisker-dev-runtime/src/hot_reload.rs::handle_patch_frame`:

1. Parse the frame, write the dylib bytes to a writable, dlopen-able
   dir (`/data/data/<pkg>/cache/whisker-patches/` on Android, `$TMPDIR`
   elsewhere), and rewrite `table.lib` to that local path.
2. Park the JumpTable in a single-slot `PENDING` mutex
   (most-recent-wins).
3. Wake the runtime (`whisker_runtime::host_wake::wake_runtime()`) so a
   frame gets scheduled even when no signal is dirty.

The Lynx TASM thread drains `PENDING` at the top of its tick. In
`whisker-driver/src/lynx/bootstrap.rs::tick_frame`:

```text
apply_pending_hot_patch()  // take_pending_patch() + subsecond::apply_patch
  → if non-empty, remount_components_for(&patched)
reactive_flush(); run_until_stalled(); reactive_flush(); flush_mounts();
renderer_flush();
```

`subsecond::apply_patch` is called **before** any user code that might
itself call `subsecond::call` is on the stack — the only safe window to
swap dispatchers. It returns the list of host fn pointers that were
rewritten; `remount_components_for` then disposes and re-mounts every
`#[component]` whose body was patched, so structural edits (new
elements, new signals) reflect on screen. State local to a remounted
component is lost; state above the remount point survives.

### Full remount — Hot Reload's escalation path

Hot Reload has two on-device reflection strategies: the per-component
remount above (state above the remount point survives), and a **full
remount** (everything resets). Both are Hot Reload — same patch
wire format, same sub-second apply, no reinstall — the choice is made
per patch, on-device, after `apply_patch`.

Per-component remount re-runs `#[component]` bodies, never `app()`
itself (`app_fn` runs once at bootstrap). Two patch shapes therefore
used to apply without rendering: an edit to the `app()` body (top-level
`provide_context` values, which root component is mounted), and a patch
in an app with no top-level `#[component]` at all (nothing registered
in `fn_ptr_mounts`). `maybe_full_remount` in `bootstrap.rs` escalates
those to a full re-run:

- `#[whisker::main]` bakes an FNV-1a hash of the app fn's tokens into
  a generated `__whisker_app_body_hash` fn, read through the same
  subsecond dispatch as the app body (`call_app_hash`). After a patch,
  a changed value means the user edited `app()` itself.
- `remount_components_for` returns `RemountStats`; `remounted == 0`
  with a non-empty patch means nothing on screen could reflect it,
  and `layout_changed > 0` means the props-layout gate below refused
  one or more sites.

### The props-layout gate

A remount re-runs the site's *stored* body closure (created at mount
time, possibly by pre-patch code) and relies on subsecond dispatch to
land in the patched inner body. That transmutes the stored closure's
captured environment into what the new code expects — sound only if
the capture layout didn't move. Two guards make it sound:

- **Forced captures.** The `#[component]` macro pins the inner
  closure's environment to *all* props (a `let _ = (&a, &b, …)` at
  the top of the dispatched closure), so the layout is a function of
  the props signature alone — not of which props the body happens to
  reference this week.
- **Layout hash.** The macro bakes `fnv1a64(props signature tokens)`
  into a generated `__whisker_props_hash` fn, folding in each prop
  type's `size_of`/`align_of` at runtime (so a change to a prop
  type's *definition* — struct fields behind an unchanged signature —
  also shifts the patch's value). The mount records the value; after
  a patch, `remount_components_for` re-reads it through dispatch and
  **refuses** any site whose value moved, reporting it in
  `RemountStats::layout_changed`. The bootstrap escalates those to a
  full remount, where fresh patched code rebuilds all props from
  scratch.

Residual gap: a prop type whose definition changes without moving its
size or alignment (e.g. two `u32` fields swapped) still slips through
the hash. The signature-token part catches renames/retypes at the
declaration; byte-identical-layout semantic swaps are on the user.

Either condition triggers: detach the page's children, dispose the
current *run owner* (a per-`app()`-run child of the persistent root
owner — contexts, signals, effects all cascade), re-invoke the kept
`app_fn` (`FnMut` since the full-remount path landed) under a fresh
run owner, and append
the new content to the same fixed page. All reactive state is lost by
design, but the process — and the dev-session WebSocket — survive, so
it's still sub-second, unlike a Full Reload reinstall.

The vendored `whisker-subsecond` (`[lib] name = "subsecond"`,
`crates/whisker-subsecond/`) is a fork of Dioxus's subsecond 0.7.9. The
one change: upstream anchors its ASLR-slide lookup on
`dlsym(RTLD_DEFAULT, "main")`, which is ambiguous in Whisker's
dylib-based Android runtime (multiple `main` symbols can coexist in one
linker namespace). The fork anchors on the unique `whisker_aslr_anchor`
symbol that `#[whisker::main]` emits (`whisker-macros/src/lib.rs`:
`#[no_mangle] pub extern "C" fn whisker_aslr_anchor() -> c_int { 0 }`).

## Why dylib, not cdylib or staticlib

The user crate is built as a Rust **`dylib`** (still a `.so` on Android,
`.dylib` on iOS), not `cdylib` and not `staticlib`:

- **`cdylib` strips symbols.** rustc unconditionally injects
  `-Wl,--exclude-libs,ALL` for `cdylib`, which removes every mangled
  Rust symbol from `.dynsym` (a cdylib is meant to expose only its
  `#[no_mangle] extern "C"` C-ABI surface). subsecond resolves patch
  references against the host's *dynamic* symbol table, so it needs
  those mangled symbols present. Switching `cdylib → dylib` took the
  hello-world `.dynsym` from ~175 entries to ~2000.
- **`staticlib` has no `.dynsym` at all.** subsecond's whole model —
  `apply_patch` rewriting host fn pointers and the patch dylib
  resolving symbols against the host — requires a real dynamic symbol
  table. A static archive can't provide one.

One side effect of the `dylib` switch: rustc auto-generates a
version-script that localizes the C++ bridge's JNI exports (`Java_*`,
`JNI_OnLoad`). `whisker-build/src/android.rs::cargo_build_dylib` passes
a second `--version-script` listing those names in `global:`; lld unions
multiple anonymous version-scripts additively, re-exporting them.
`whisker-driver-sys/build.rs` no longer emits the old
`rustc-link-arg-cdylib` directives (silently dropped for non-cdylib
consumers).

## Dev-session security model

The patch channel `dlopen`s whatever bytes it receives, so on a
LAN-exposed bind an unauthenticated peer could push arbitrary native
code. Two defenses:

- **Loopback bind by default.** `whisker run --bind` defaults to
  `127.0.0.1:9876`. On Android the device reaches the host through
  `adb reverse tcp:9876 tcp:<dev_port>` (set up by the installer); the
  app reads `WHISKER_DEV_ADDR` or falls back to `127.0.0.1:9876`.
- **Per-session token.** `whisker run` generates a random 32-hex-char
  token per session (`generate_dev_token` in `whisker-cli/src/run.rs`,
  16 bytes from `/dev/urandom`). It's delivered to the device
  out-of-band:
  - **iOS Simulator / host:** the `WHISKER_DEV_TOKEN` env var, set via
    `SIMCTL_CHILD_WHISKER_DEV_TOKEN` (the `SIMCTL_CHILD_<NAME>`
    convention surfaces `<NAME>` inside the launched app).
  - **Android:** the `debug.whisker_dev_token` system property, set
    with `adb shell setprop` (the app process doesn't inherit
    adb-set env vars). `whisker-dev-runtime` reads it via
    `__system_property_get`.

The device echoes the token in its `hello`. The server validates it
(`server.rs::handle_socket`): a client starts *unauthenticated* when a
token is required and is promoted only on a matching `hello`; a missing
or mismatched token closes the connection without ever arming the patch
path. A token-less server (`dev_token == None`, e.g. tests) runs open by
default. While unauthenticated, broadcast patches are dropped (not
buffered) for that client — the device re-receives the full JumpTable on
the next save anyway.

## Known boundaries

- **iOS hardware is unsupported** for Hot Reload: `mmap(PROT_WRITE |
  PROT_EXEC)` is blocked by Apple's W^X policy, so `apply_patch` can't
  run. Targets are macOS host, Android, and the iOS Simulator.
- **`Cargo.toml` / `Cargo.lock` edits** always need a Full Reload —
  the patcher can't reload dependencies.
- **Multi-crate change batches** can't be expressed as a single patch
  (one crate per patch), so they prompt for a Full Reload.
- **Per-component remount loses local state.** A patch that changes a
  component's structure re-mounts it; signals owned by that component
  reset. State held above the remount point (context, parent signals)
  survives.
