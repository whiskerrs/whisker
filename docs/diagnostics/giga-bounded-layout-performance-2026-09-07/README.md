# GIGA Android: bounded layout reuse and intrinsic Text measurement

2026-09-07. Production implementation: `9d70dde3`; baseline: `7d76eed8` (the earlier retained-state improvements). Both are Android debug builds. This comparison isolates the two changes proposed in [the investigation](../giga-layout-investigation-2026-09-07/README.md), rather than attributing all preceding improvements to this patch.

## Implementation

- A private layout adapter invokes unmodified Taffy 0.13.0 through its public low-level algorithms. It retains at most 32 exact `LayoutInput` / size pairs per node for `ComputeSize`; after filling, new entries replace old entries in FIFO order. Taffy's existing cache still handles the fallback and final layout. Size entries are allocated lazily and shared across snapshots until mutation.
- Taffy's dirty propagation clears the extra size cache for changed nodes and ancestors before computation. Hidden layout clears both caches; subtree deletion removes retained layout state. No rounding of width constraints or application-specific exceptions are introduced.
- Text requests canonicalize unused available height to max-content, including the actual provider request. Known height, width constraints, max-lines and style/environment identity remain effective. Other measurement kinds preserve both axes. Desktop intrinsic Text measurement now follows the same contract as Android, iOS and Web.

The cache adds bounded per-node storage and a linear lookup of at most 32 inputs. The layout adapter also retains unrounded geometry to use the public Taffy API. Snapshot mutation and invalidation behavior are covered by tests; this run does not quantify total process-memory growth. No Taffy fork or new dependency is used.

## Computational work

On returning from Reader to the same Group, the measured Text update has the following sequence. Both rounds produced identical request and miss counts.

| Measure | Baseline | Combined implementation |
| --- | ---: | ---: |
| Layout passes until requests settle | 4 | 3 |
| Host measurement requests | 13 / 12 / 4 / 0 = **29** | 5 / 3 / 0 = **8** |
| Taffy cache misses | 3,994 / 1,237 / 744 / 284 = **6,259** | 210 / 143 / 105 = **458** |
| Taffy root time, first / second round | 114.8 / 151.8ms | 37.7 / 21.5ms |

The miss count falls by about 92.7%. Taffy-root timings include Rust-side measurement-cache callbacks but exclude Host FFI measurement. Instrumentation adds counters, sets and per-pass logging, so these timings are diagnostic, not frame-time guarantees. The original experiment's unbounded cache is not shipped.

## Frame controls without instrumentation

Android emulator `emulator-5554`, API 37, 1080×2400. Home → Kingdom Group → Reader → Group → six list swipes → Home, with 7-second captures per transition and 10 seconds for scrolling. These APKs have no diagnostic helper or active profiler. No concurrent build ran during these controls.

Order: fixed round 1 → baseline round 1 → fixed round 2 → baseline round 2. Later rounds reinstall and relaunch the same app while retaining data. Fixed round 1 used the already launched app, so startup/cache conditions are not perfectly identical. Each capture checks stable PID and nonzero rendered frames. Values below are Android `dumpsys gfxinfo` p95 histogram estimates, not Rust tick duration.

| Operation | Baseline round 1 / 2 | Fixed round 1 / 2 |
| --- | ---: | ---: |
| Open Group | 133 / 61ms | 65 / 101ms |
| Open Reader | 65 / 65ms | 93 / 69ms |
| Return from Reader | 200 / 200ms | 200 / 133ms |
| Scroll chapter list | 400 / 350ms | 200 / 150ms |
| Return to Home | 101 / 81ms | 133 / 117ms |

List scrolling improved in both paired captures. The transition measurements are mixed; this does not establish that every transition improved, and the Home-return samples are slower. Samples are small and emulator scheduling, image/native work and reactive updates remain contributors. Frames still exceed 16ms.

### Repeated Reader transitions

An additional six warm open/back pairs use the same chapter (885), fixed then baseline. The baseline's first automated warm-up produced zero frames at the first capture and was excluded; the Group was visually confirmed before retrying. First and last open/back screenshots confirm the target screens, and each valid capture checks PID and frame count.

| Operation | Baseline median p95 (range across six captures) | Fixed median p95 (range) |
| --- | ---: | ---: |
| Open Reader | 150ms (117–150ms) | 95ms (81–150ms) |
| Return from Reader | 150ms (69–200ms) | 117ms (77–150ms) |

These repeats do not reproduce a consistent Reader-open regression. They are sequential emulator measurements, not a randomized benchmark or proof that the transition always improves. Raw values are in `reader-repeats.csv`.

## Validation

- Local engine 80, layout 29, protocol 74 unit tests and 8 Desktop Text tests passed.
- Cache regression: disabling the added cache produces 36 measurements instead of the expected 12 for repeated colliding constraints. Tests also cover bounded eviction, dirty invalidation and snapshot isolation.
- Differential geometry tests compare the adapter with stock Taffy across Flex, Grid, Block, FlowRoot and hidden layouts, style mutations and viewport changes.
- Text regression: unused available heights formerly produce four requests; now one. Tests preserve distinct known heights, widths and non-Text measurement kinds, and verify Desktop wrapping/max-lines behavior.
- Combined engine/layout/protocol coverage is 100% for lines, functions and regions. CI also runs foundation crates separately: two layout getters used only by engine tests initially failed that isolated gate. Layout's own tests now cover them, and isolated layout coverage passed locally at 100%.
- On implementation commit `9d70dde3`, CI passed workspace tests, clippy, formatting, docs, dependency policy, MSRV 1.88, mobile ABI/link checks, and Android/iOS/Web/Desktop Host conformance. The follow-up test-only commit addresses the isolated coverage failure.

## Reproduction and artifacts

- `frame-controls.csv`: all 20 profiler-free captures.
- `layout-passes.csv`: all passes from the ten instrumented fixed-build captures. Baseline passes are in the preceding investigation.
- `artifacts.txt`: SHA-256 identities of the baseline, uninstrumented fixed and diagnostic fixed APKs.
- Local raw captures, scripts and traces: `/tmp/giga-layout-improvements`; uninstrumented fixed APK: `/tmp/giga-layout-improvements-plain/profile.apk`; baseline APK: `/tmp/giga-shared-state-build/profile.apk`.
- `control.py <label>` runs one five-operation control round from Home. `repeat_reader.py <label>` warms the same chapter and records six Reader-open/back pairs. Scripts depend on the connected emulator and retained GIGA test data; screenshots and content are not committed.
- GIGA's manifest, lockfile and temporary SDK links are restored. The final emulator install is the uninstrumented improved build with app data retained. No dev server or profiler is left running.
