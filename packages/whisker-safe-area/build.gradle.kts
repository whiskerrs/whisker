// Gradle build for `whisker-safe-area`'s Android half.
//
// Mirrors `whisker-image` / `whisker-video`'s shape: one KSP-processed
// module subproject with sources under `android/src/main/kotlin`,
// depending on Whisker's runtime + AndroidX core for the
// `WindowInsetsCompat` API.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.safe_area"
    compileSdk = 34

    defaultConfig {
        // AndroidX core's `WindowInsetsCompat` works back to API 14;
        // we keep Whisker's standard minSdk = 21 baseline.
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
    arg("whisker.moduleName", "WhiskerSafeArea")
    arg("whisker.crateName", "whisker-safe-area")
}

dependencies {
    implementation(project(":module"))
    ksp("rs.whisker:ksp")

    // `WindowInsetsCompat` + `ViewCompat.setOnApplyWindowInsetsListener`
    // — the AndroidX wrappers that paper over the API-30 introduction
    // of typed inset categories. Keeping the dep on `core-ktx`
    // (rather than vanilla `core`) lets us use the inline
    // `getInsets(systemBars() or displayCutout())` syntax.
    implementation("androidx.core:core-ktx:1.13.1")
}
