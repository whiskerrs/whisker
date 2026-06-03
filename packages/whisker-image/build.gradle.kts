// Gradle build for `whisker-image`'s Android half.
//
// Mirrors `whisker-video`'s shape: one KSP-processed module subproject
// with sources under `android/src/main/kotlin`, depending on Whisker's
// runtime + Coil for URL-based image loading.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.image"
    compileSdk = 34

    defaultConfig {
        // Coil 2.7 needs API 21+, which lines up with Whisker's
        // baseline. We don't drop to anything lower — Coil 1.x is
        // unmaintained and lacks the coroutine-native API the
        // ImageView code below leans on.
        minSdk = 21
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    // Source set redirection so AGP only scans the `android/` subtree;
    // Rust's `src/` next to this file stays out of the Kotlin
    // compiler's view.
    sourceSets {
        getByName("main") {
            kotlin.srcDirs("android/src/main/kotlin")
        }
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerImage")
    arg("whisker.crateName", "whisker-image")
}

dependencies {
    implementation(project(":module"))
    ksp("rs.whisker:ksp")

    // Coil 2.7 — Kotlin-first, coroutine-native image loader. Base
    // artifact covers PNG / JPEG / static WebP (Android 14+ native
    // for WebP / AVIF — older OS WebP falls through to the GIF
    // backport). GIF / SVG / animated WebP backport are separate
    // artifacts and intentionally not pulled here; the base module
    // stays slim, consumers add what they need.
    implementation("io.coil-kt:coil:2.7.0")
}
