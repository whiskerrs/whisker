plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    // Phase 7-Φ.H.2: KSP processor that discovers @WhiskerElement
    // annotations across the user app's compilation (including the
    // module-crate Kotlin sources staged under `whisker_modules/`)
    // and generates `rs.whisker.runtime.generated.WhiskerModuleBehaviors`.
    // The KSP processor itself lives in `packages/whisker-android-ksp/`
    // and is pulled in via the composite-build entry added in
    // `settings.gradle.kts`.
    //
    // Version is pinned to match `org.jetbrains.kotlin.android`
    // 2.0.21 — KSP releases follow the Kotlin major.minor.patch
    // with a trailing `-1.0.N` ABI suffix. Bump in lockstep with
    // the Kotlin version.
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
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
            // Whisker module-system v1 (Phase 7-Φ.C): module crates'
            // Android Kotlin sources are staged into `whisker_modules/`
            // by `whisker-build::android::stage_module_kotlin_sources`.
            // We include it here as a Kotlin source root so gradle's
            // compileDebugKotlin / compileReleaseKotlin tasks pick
            // them up alongside `src/main/kotlin/`.
            //
            // Empty when no module declares android.kotlin_sources —
            // gradle is fine with non-existent source roots.
            kotlin.srcDirs("src/main/kotlin", "src/main/whisker_modules")
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
    // `@WhiskerElement` annotation + KSP processor (Phase 7-Φ.H.2).
    // Both targets live in the composite-build referenced from
    // `settings.gradle.kts`. The annotation dep is `implementation`
    // so module-crate Kotlin sources can resolve the
    // `rs.whisker.annotations.WhiskerElement` symbol; the `ksp(...)`
    // dep wires the processor into Kotlin compilation so it sees
    // the annotation applications and emits the registry.
    implementation("rs.whisker:annotations")
    ksp("rs.whisker:ksp")
}
