// Gradle build for `whisker-video`'s Android half (Phase 7-Φ.H.2.6).

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.video"
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

ksp {
    arg("whisker.moduleName", "WhiskerVideo")
    arg("whisker.crateName", "whisker-video")
}

dependencies {
    // Phase J — single Whisker runtime dep. `ksp("rs.whisker:ksp:0.1.0")`
    // stays separate (it is a build-time processor, not on the
    // runtime classpath). Phase M (Issue #59) dropped the
    // `:annotations` JAR: the KSP processor finds Module subclasses
    // by inheritance now, so no marker annotation is needed.
    implementation("rs.whisker:whisker-module-android:0.1.0")
    ksp("rs.whisker:ksp:0.1.0")

    // AndroidX Media3 — modern replacement for the deprecated
    // android.widget.VideoView / MediaPlayer pair. ExoPlayer is
    // the underlying player; PlayerView is the view widget.
    // Version pinned to Media3 1.4.1 (stable, mid-2024).
    implementation("androidx.media3:media3-exoplayer:1.4.1")
    implementation("androidx.media3:media3-ui:1.4.1")
}
