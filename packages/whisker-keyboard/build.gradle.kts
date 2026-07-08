// Gradle build for `whisker-keyboard`'s Android half.
//
// Mirrors `whisker-safe-area`'s shape: one KSP-processed module
// subproject with sources under `android/src/main/kotlin`, depending on
// Whisker's runtime + AndroidX core for the `WindowInsetsCompat.Type.ime()`
// API.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.keyboard"
    compileSdk = 34

    defaultConfig {
        // AndroidX core's `WindowInsetsCompat.Type.ime()` works back to
        // API 21 (it reads the platform IME inset on 30+, and best-effort
        // below); keep Whisker's standard minSdk = 21 baseline.
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
    // Rust's `src/` next to this file stays out of the Kotlin compiler's
    // view.
    sourceSets {
        getByName("main") {
            kotlin.srcDirs("android/src/main/kotlin")
        }
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerKeyboard")
    arg("whisker.crateName", "whisker-keyboard")
}

dependencies {
    // 0.1.6 is the first SDK release carrying `WhiskerInsetsDispatcher`
    // (the shared decor-view inset multiplexer this module subscribes
    // to). Requires cutting `sdk-v0.1.6`; see the module refactor commit.
    implementation("rs.whisker:whisker-module-android:0.1.6")
    ksp("rs.whisker:ksp:0.1.0")

    // `WindowInsetsCompat.Type.ime()` + `ViewCompat.setOnApplyWindowInsetsListener`
    // — the AndroidX wrappers that expose the IME inset uniformly across
    // API levels.
    implementation("androidx.core:core-ktx:1.13.1")
}
