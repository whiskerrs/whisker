# Android debug frame optimization — 2026-09-07

## Changes and confirmed causes

Layout observers used by List mounted rows and resized spacers outside a renderer mutation batch. Each style application could clone the retained surface independently. A runtime regression test mounting 24 rows and updating a spacer reproduces 50 surface snapshots before the change and one afterward. Layout and batch-end callbacks now share a mutation batch. This retains the existing atomic style-commit behavior; a failed commit restores styles and closes the batch.

Changing a route wrapper's opacity also resolved styles and captured motion targets throughout its subtree. These operations now skip clean descendants when the inherited style is unchanged. Explicitly dirty descendants remain reachable even if an ancestor is also dirty. Inherited changes still propagate, and a later inherited change in the same batch expands the original motion capture while preserving its earlier targets. A parent with 24 children now requires one style resolution and one motion snapshot for an opacity-only change, instead of 25 each.

These changes are internal to the Rust runtime. They add no public APIs or Kotlin/Swift changes. The full retained-surface clone used for atomic application remains; this work does not remove every source of per-frame cost.

## Android comparison

GIGA 3.0.0 (version code 26), Rust debug builds, Whisker 0.13.7, Android emulator `sdk_gphone16k_arm64` (API 37, 1080×2400). Both APKs used the same GIGA sources and SDK. Only the two runtime implementation files changed between baseline and optimized APKs. The temporary Cargo patches were restored after building.

Each APK was installed and launched, then the same sequence was measured: open Kingdom, open the reader, return to the group, swipe the chapter list six times, and return Home. `dumpsys gfxinfo` was reset before each action. No builds or profilers ran concurrently during this comparison. Screenshots were used to check the target screens. These are approximate histogram percentiles from one valid cycle per APK, not a statistically established device-wide speedup.

| Action | Baseline p95 | Optimized p95 | Baseline jank | Optimized jank |
| --- | ---: | ---: | ---: | ---: |
| Open group | 44 ms | 25 ms | 5.05% | 4.08% |
| Open reader | 48 ms | 28 ms | 8.25% | 5.88% |
| Reader → group | 97 ms | 34 ms | 35.71% | 15.62% |
| Scroll chapters | 48 ms | 48 ms | 30.05% | 35.48% |
| Group → Home | 117 ms | 36 ms | 36.00% | 14.71% |

Navigation improved in this comparison, particularly returning to an existing screen. Chapter scrolling has no demonstrated end-to-end improvement: p95 was unchanged and the measured jank fraction increased. It remains an open performance problem. Opening a group still reached 150 ms at p99 in both builds.

Earlier runs showed much larger improvements, but reinstalling and comparing without concurrent work reduced the difference. Those earlier figures are excluded from the conclusions. Additional Perfetto navigation captures missed their intended screens during loading and included a disposed-signal error during activity recreation; they are invalid for performance comparison. That error's cause was not established in this optimization task. No CPU attribution claim is made from those traces. Emulator GPU percentile overflow buckets are also excluded.

The next investigation should isolate scrolling after chapter loading completes, then measure remaining surface cloning, layout, native view updates, and rendering before choosing another optimization. Physical-device verification is still needed.

## Validation and artifacts

- Runtime unit tests: 191 passed, including batching, nested dirty updates, inherited styles, motion capture expansion, and failed-commit rollback.
- GIGA List diagnostics: 4 passed.
- Surface rendering pipeline integration tests: 56 passed.
- `cargo clippy -p whisker-runtime --lib --tests -- -D warnings`: passed.
- Formatting and `git diff --check`: passed.
- Regression tests were run before applying the corresponding fix and failed on the redundant work counts (50 vs. 1 snapshots, 25 vs. 1 style resolutions).

Structured results are in `giga-android-frame-optimization-2026-09-07.json` beside this report. Local raw measurements and screenshots are in `/tmp/giga-android-frame-study/aba-{baseline,optimized}-0-*`; APKs, build scripts, and test logs are in `/tmp/giga-android-frame-optimization/`. These temporary artifacts are not committed and may be removed by the operating system.
