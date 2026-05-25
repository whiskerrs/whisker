// Gradle build for the `whisker-hello-element` module's Android
// half (Phase 7-Φ.G).
//
// Each Whisker module package is now its own Android library
// subproject. whisker-build's android sync generates a
// `settings.gradle.kts` include + sets `projectDir` to this
// directory, so the user app's gradle composite sees this module
// as `:whisker-hello-element`.
//
// Module authors are free to add their own Maven / AAR deps here.
// KSP runs per-subproject and emits a uniquely-named
// `<ModuleName>Behaviors.kt` registration helper; the user app's
// whisker-build-generated top-level aggregator imports + calls
// each one.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    // KSP version pinned to the same `<kotlin>-<abi>` pair the
    // user app uses (see `crates/whisker-cng/src/templates/android/
    // app/build.gradle.kts`). Bump in lockstep with Kotlin.
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    // Unique per-module package namespace. Conventionally
    // `rs.whisker.modules.<crate-name-flat>` so two modules can't
    // shadow each other's resources / R class.
    namespace = "rs.whisker.modules.helloelement"
    compileSdk = 34

    defaultConfig {
        minSdk = 21
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    // build.gradle.kts sits at the package root (alongside
    // Package.swift + Cargo.toml). Point the Kotlin source set at
    // the package's `android/` subdir so AGP doesn't scan the Rust
    // `src/`, and the native code stays grouped under `android/`.
    sourceSets {
        getByName("main") {
            kotlin.srcDirs("android/src/main/kotlin")
        }
    }
}

// Pass the module name to KSP so the processor can produce a
// uniquely-named `<Module>Behaviors.kt` per subproject. The
// processor reads this option via
// `environment.options["whisker.moduleName"]`.
//
// `whisker.crateName` (Phase 7-Φ.H.2) is the cargo crate name —
// used as the element-tag namespace so two unrelated module
// packages can both declare an element named `Video` without
// colliding in Lynx's behaviour registry.
ksp {
    arg("whisker.moduleName", "WhiskerHelloElement")
    arg("whisker.crateName", "whisker-hello-element")
}

dependencies {
    // Whisker runtime provides the `WhiskerValue` sealed class,
    // `WhiskerModuleRegistry`, plus the Lynx AAR (via api(…))
    // for `LynxUI`, `LynxComponentRegistry`, etc.
    // Phase J — single Whisker runtime dep. `:module` re-exports
    // `rs.whisker:annotations` transitively, so no separate dep on
    // the annotation JAR is needed. `ksp("rs.whisker:ksp")` stays
    // separate (it is a build-time processor, not on the runtime
    // classpath).
    implementation(project(":module"))
    // `@WhiskerComponent` / `@WhiskerModule` annotations + KSP
    // processor. Resolved through the user-app gradle's
    // composite-build entry for `platforms/android/ksp`.
    ksp("rs.whisker:ksp")
}
