// Phase J — `whisker-module` minimal Android surface for
// third-party Whisker modules. Carved out of `whisker-runtime` so
// modules pull in just the types they need (`WhiskerValue`,
// `WhiskerUI` / `WhiskerContext` typealiases, `WhiskerApplication`)
// without dragging in the host-side `WhiskerActivity`,
// `WhiskerView`, or `WhiskerModuleRegistry`.
//
// The module-author dep surface is exactly this AAR + the
// composite-build `rs.whisker:ksp` for build-time inheritance-
// based discovery. Phase M (Issue #59) dropped the
// `:annotations` JAR — discovery is now subclass-of-Module so
// no marker annotation is required.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    // AGP namespace MUST be distinct from `:whisker-runtime`'s
    // (which also lives under the `rs.whisker.runtime` Kotlin
    // package) — otherwise AGP errors out with "Namespace is used
    // in multiple modules". The Kotlin sources inside can still
    // declare `package rs.whisker.runtime` for stable external
    // imports; this AGP-level namespace just disambiguates the
    // R class.
    namespace = "rs.whisker.runtime.moduleapi"
    compileSdk = 34

    defaultConfig {
        minSdk = 24

        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    // `api` (not `implementation`) so consuming apps + modules can
    // see LynxUI / LynxContext / LynxView types that the
    // `Whisker*` typealiases in `WhiskerLynxAliases.kt` resolve to.
    api(":LynxAndroid@aar")
    api(":LynxBase@aar")
    api(":LynxTrace@aar")
    api(":ServiceAPI@aar")
    api("org.lynxsdk.lynx:primjs:3.7.0")

    // No annotation re-export needed (Phase M / Issue #59): a
    // module's `build.gradle.kts` depends on this AAR alone for
    // runtime types; the `ksp(...)` processor stays separate as
    // a build-time dep.
}
