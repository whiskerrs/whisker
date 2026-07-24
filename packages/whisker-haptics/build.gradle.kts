// Gradle build for the local `whisker-haptics` module's Android half.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.haptics"
    compileSdk = 34

    defaultConfig {
        // `VibratorManager` needs API 31; predefined `VibrationEffect`s
        // need API 29 — both are branched at runtime in
        // `HapticsModule.kt`, falling back to `Vibrator.vibrate(ms)`
        // below that. No extra floor bump needed beyond the app's own
        // minSdk.
        minSdk = 21
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    // build.gradle.kts sits at the package root (alongside Package.swift
    // + Cargo.toml). Point the Kotlin source set at the package's
    // `android/` subdir so AGP doesn't scan the Rust `src/`.
    sourceSets {
        getByName("main") {
            kotlin.srcDirs("android/src/main/kotlin")
        }
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerHaptics")
    arg("whisker.crateName", "whisker-haptics")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.6")
    ksp("rs.whisker:ksp:0.1.0")
    // No extra native dep: `Vibrator`/`VibratorManager` are platform
    // APIs — see `src/lib.rs`'s doc comment.
}
