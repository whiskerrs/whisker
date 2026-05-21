# whisker-native-runtime (Android)

Phase 7 skeleton. Final shape:

- `runtime/` — Kotlin library AAR. Houses:
  - `@WhiskerModule`, `@WhiskerElement`, `@WhiskerMethod`,
    `@WhiskerProp`, `@WhiskerEvent`, `@WhiskerSubscription`
    annotation classes (open class bodies, no behaviour).
  - `WhiskerView<V : View>` base class (Lynx `LynxUI` subclass
    wrapping the user's `View`).
  - `WhiskerContext`, `WhiskerCallback`, `WhiskerEventEmitter`
    helper types.
- `ksp-processor/` — KSP `SymbolProcessor` that scans for
  `@Whisker*`-annotated classes and emits `LynxUI` /
  `LynxNativeModule` subclasses + `@LynxBehavior` /
  `@LynxMethod` registrations underneath.

Both subprojects are stubs today (Phase 7-A.3). Real implementations
land with Phase 7-B.5 (element runtime) and Phase 7-C (module
runtime).

## Building (manual)

```sh
cd runtime-platforms/android
./gradlew assembleRelease
```

The autolink layer (`whisker-cng`, Phase 7-D) wires this into the
user app's `gen/android/` via `includeBuild` — no `./gradlew`
invocation by the user.
