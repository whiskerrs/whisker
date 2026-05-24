plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    // Phase 7-Φ.G: KSP is no longer applied at the app level. Each
    // Whisker module package is now its own Android library subproject
    // with KSP running per-subproject — see
    // `packages/whisker-*/build.gradle.kts`. The user app no longer
    // sees `@WhiskerComponent` / `@WhiskerModule` annotations directly
    // (they're inside the subproject classpaths), so it has nothing
    // to process. The whisker-build-generated
    // `WhiskerModuleBehaviors.kt` (under
    // `src/main/whisker_generated/`) imports each subproject's
    // per-module behaviors object and chains the `registerAll()`
    // calls.
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
            // Phase 7-Φ.G: `src/main/whisker_generated/` holds the
            // whisker-build-generated
            // `rs.whisker.runtime.generated.WhiskerModuleBehaviors`
            // aggregator that imports each Whisker module's
            // KSP-generated `<ModuleName>Behaviors` object and
            // chains the `registerAll()` calls. The actual module
            // sources live in their own subprojects now — we don't
            // stage them into the app's source set anymore.
            //
            // Empty when no Whisker module deps — gradle is fine
            // with non-existent source roots.
            kotlin.srcDirs("src/main/kotlin", "src/main/whisker_generated")
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

// Phase 7-Φ.G: whisker-build emits per-module
// `implementation(project(":<crate-name>"))` deps into
// `whisker_module_deps.gradle.kts` (under the gradle root) so the
// list refreshes when cargo deps change. Applied with this build
// script's `Project` as the receiver, so `dependencies { ... }`
// blocks inside it register against this `:app` target.
val whiskerModuleDeps = file("${rootProject.projectDir}/whisker_module_deps.gradle.kts")
if (whiskerModuleDeps.exists()) {
    apply(from = whiskerModuleDeps)
}
