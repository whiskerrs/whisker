// Gradle build for `whisker-asset`'s Android half.
//
// Mirrors `whisker-audio` / `whisker-image`: one KSP-processed module
// subproject with sources under `android/src/main/kotlin`, depending
// only on Whisker's runtime (no third-party libs — the module is
// startup-only and view-less).
//
// Note on the asset base: unlike iOS, the Android resolver base is a
// compile-time constant (`file:///android_asset/whisker/`), so it is
// installed from RUST via an `.init_array` constructor in
// `whisker-asset`'s lib (`install_android_base`) that runs at `.so`
// load — see `crates/whisker-asset/src/lib.rs`. This Kotlin module
// therefore carries no startup logic of its own; it exists so the
// `[package.metadata.whisker]` marker wires a real Android subproject
// (KSP registration, parity with the iOS package) and as the home for
// any future native asset work.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.asset"
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
    // Package.swift + Cargo.toml). Point the Kotlin source set at the
    // package's `android/` subdir so AGP doesn't scan the Rust `src/`.
    sourceSets {
        getByName("main") {
            kotlin.srcDirs("android/src/main/kotlin")
        }
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerAsset")
    arg("whisker.crateName", "whisker-asset")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.0")
    ksp("rs.whisker:ksp:0.1.0")
}
