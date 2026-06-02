// Gradle build for the `whisker-router` module's Android half.
//
// Step 2 of the Android predictive-back work: turn whisker-router
// into a module package so we can declare a native module that
// listens to `OnBackInvokedCallback` and publishes back-button
// events to Rust via `whisker-module`'s event system.
//
// Mirrors `packages/whisker-hello-element/build.gradle.kts`. The
// Cargo.toml's `[package.metadata.whisker]` table flips the
// crate into module-package mode; whisker-build's android sync
// generates a `settings.gradle.kts` include + sets `projectDir`
// to this directory, so the user app's gradle composite sees
// this module as `:whisker-router`.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    // KSP version pinned to the same `<kotlin>-<abi>` pair the
    // user app uses. Bump in lockstep with Kotlin.
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.router"
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
    // Cargo.toml). Point the Kotlin source set at `android/src/main/
    // kotlin` so AGP doesn't scan the Rust `src/`.
    sourceSets {
        getByName("main") {
            kotlin.srcDirs("android/src/main/kotlin")
        }
    }
}

// Pass the module name + crate name to KSP so the processor emits
// a uniquely-named `WhiskerRouterBehaviors.kt` registration helper.
ksp {
    arg("whisker.moduleName", "WhiskerRouter")
    arg("whisker.crateName", "whisker-router")
}

dependencies {
    implementation(project(":module"))
    // androidx.activity.OnBackPressedDispatcher / -Callback for
    // PredictiveBackModule. 1.8+ is the floor that wires
    // OnBackPressedDispatcher into the platform's OnBackInvoked
    // path on API 33+ — older androidx routes the legacy path.
    implementation("androidx.activity:activity:1.8.2")
    ksp("rs.whisker:ksp")
}
