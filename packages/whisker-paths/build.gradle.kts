// Gradle build for the `whisker-paths` module's Android half.
// See `whisker-secure-store/build.gradle.kts` for the architectural
// rationale.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.paths"
    compileSdk = 34

    defaultConfig {
        // Only needs Context.getCacheDir/getFilesDir, available since
        // API 1 — floor at 21 to match whisker-local-store.
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
    arg("whisker.moduleName", "WhiskerPaths")
    arg("whisker.crateName", "whisker-paths")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.0")
    ksp("rs.whisker:ksp:0.1.0")
}
