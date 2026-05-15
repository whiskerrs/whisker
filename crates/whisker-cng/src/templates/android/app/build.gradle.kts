plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "{{android_application_id}}"
    compileSdk = {{android_target_sdk}}
    ndkVersion = "21.1.6352462"

    defaultConfig {
        applicationId = "{{android_application_id}}"
        minSdk = {{android_min_sdk}}
        targetSdk = {{android_target_sdk}}
        versionCode = {{build_number}}
        versionName = "{{version}}"

        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    // The Rust dylib (lib{{rust_lib_name}}.so) is dropped into this
    // dir by `whisker run` / `whisker build`.
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    buildTypes {
        getByName("debug") {
            isMinifyEnabled = false
            // Keep symbols readable for ndk-stack.
            packaging {
                jniLibs.keepDebugSymbols += listOf("**/lib{{rust_lib_name}}.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = false
        }
    }
}

dependencies {
    implementation(project(":whisker-runtime"))
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.core:core-ktx:1.13.1")
}
