// Gradle build for the local `whisker-status-bar` module's Android half.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.status_bar"
    compileSdk = 34

    defaultConfig {
        // `WindowInsetsControllerCompat` (androidx.core) backports
        // status-bar hide/show to API 21, so no floor bump beyond the
        // app's own minSdk.
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
    arg("whisker.moduleName", "WhiskerStatusBar")
    arg("whisker.crateName", "whisker-status-bar")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.6")
    ksp("rs.whisker:ksp:0.1.0")
    // `WindowInsetsControllerCompat` / `WindowCompat` live in
    // androidx.core — the runtime already depends on it transitively,
    // but declare it so this module builds standalone too.
    implementation("androidx.core:core-ktx:1.13.1")
}
