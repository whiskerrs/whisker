// Gradle build for the `whisker-secure-store` module's Android half.
// See `whisker-local-store/build.gradle.kts` for the architectural
// rationale.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.securestore"
    compileSdk = 34

    defaultConfig {
        // Tink's Android Keystore master key (envelope-encrypting the
        // keyset) requires API 23. Below that the keyset would be stored
        // unwrapped, defeating the point — so this module floors at 23
        // (vs. whisker-local-store's 21).
        minSdk = 23
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
    arg("whisker.moduleName", "WhiskerSecureStore")
    arg("whisker.crateName", "whisker-secure-store")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.0")
    ksp("rs.whisker:ksp:0.1.0")

    // Google Tink — AES-256-GCM payload crypto with an Android
    // Keystore-wrapped keyset. Google's recommended replacement for the
    // deprecated androidx.security EncryptedSharedPreferences.
    implementation("com.google.crypto.tink:tink-android:1.13.0")
}
