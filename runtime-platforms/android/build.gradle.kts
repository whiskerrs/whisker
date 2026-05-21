// WhiskerNativeRuntime (Android) — Phase 7-A.3 skeleton.
//
// Hosts the `@WhiskerModule` / `@WhiskerElement` annotation classes
// + the `WhiskerView<T>` base class + the KSP processor that
// rewrites Whisker-namespaced annotations into the underlying Lynx
// `@LynxBehavior` / `@LynxMethod` / `@LynxProp` registrations.
//
// Today this file declares an empty Android library so:
//   - `./gradlew :runtime:assembleRelease` succeeds.
//   - Module crates can declare a Maven coordinate against this
//     module before its real implementation lands.
//
// Real classes + KSP processor arrive with Phase 7-B.5 (element
// side) and Phase 7-C (module side). For now the skeleton only
// proves the project layout compiles.

plugins {
    id("com.android.library") version "8.2.0" apply false
    kotlin("android") version "1.9.20" apply false
}
