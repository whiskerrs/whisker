// `whisker-annotations` — the public `@WhiskerElement` annotation
// surface. Pure Kotlin/JVM library so module-crate code that
// declares `@WhiskerElement("x-tag")` on its `LynxUI` subclass has
// a lightweight dep (no Android Gradle Plugin needed for an
// annotation type).
//
// Consumed by:
//   - Module crates' Kotlin sources (e.g.
//     `packages/whisker-hello-element/src/android/`) via
//     `implementation(...)`.
//   - The companion `:ksp` processor at compile + processor-run
//     time, so the processor can recognise the annotation's KSP
//     symbol declaration when scanning user code.

plugins {
    kotlin("jvm")
    `java-library`
}

kotlin {
    jvmToolchain(17)
}
