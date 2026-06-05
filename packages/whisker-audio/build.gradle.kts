// Gradle build for `whisker-audio`'s Android half.
//
// Mirrors `whisker-video`'s shape (Media3 ExoPlayer-backed) but
// without the PlayerView UI — audio playback has no on-screen
// surface, so the view is a zero-size placeholder and the ExoPlayer
// instance attaches to nothing.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.audio"
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
    arg("whisker.moduleName", "WhiskerAudio")
    arg("whisker.crateName", "whisker-audio")
}

dependencies {
    implementation(project(":module"))
    ksp("rs.whisker:ksp")

    // AndroidX Media3 — modern Player API. We use ExoPlayer (the
    // engine) directly without PlayerView (the visual chrome),
    // since audio doesn't need a render surface.
    implementation("androidx.media3:media3-exoplayer:1.4.1")
    implementation("androidx.media3:media3-common:1.4.1")
}
