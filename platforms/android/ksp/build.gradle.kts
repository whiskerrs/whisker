// Root build script — no plugins applied here; per-module
// `build.gradle.kts` brings them in. Subprojects share the same
// Kotlin version as the host Whisker runtime (`platforms/android` →
// `org.jetbrains.kotlin.android` 2.0.21) so a KSP processor compiled
// against a different Kotlin version doesn't trip a runtime ABI
// mismatch when it runs against the user app's compilation.

plugins {
    kotlin("jvm") version "2.0.21" apply false
}

allprojects {
    group = "rs.whisker"
    version = "0.1.0"
}
