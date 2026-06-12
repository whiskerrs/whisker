// Gradle build for `whisker-input`'s Android half.
//
// Mirrors `whisker-image`'s shape: one KSP-processed module subproject
// with sources under `android/src/main/kotlin`, depending on Whisker's
// runtime. No extra UI-library dependencies — the EditText we wrap is
// part of the Android framework.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.input"
    compileSdk = 34

    defaultConfig {
        // `textCursorDrawable` tint (API 29) is used best-effort for
        // caret-color; the fallback path works on API 21+, which aligns
        // with Whisker's baseline.
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
    arg("whisker.moduleName", "WhiskerInput")
    arg("whisker.crateName", "whisker-input")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.0")
    ksp("rs.whisker:ksp:0.1.0")
}
