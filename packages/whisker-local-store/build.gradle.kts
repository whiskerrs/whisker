// Gradle build for the `whisker-local-store` module's Android
// half (Phase 7-Φ.G). See `whisker-hello-element/build.gradle.kts`
// for the architectural rationale.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.localstore"
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

    sourceSets {
        getByName("main") {
            kotlin.srcDirs("src/android")
        }
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerLocalStore")
    arg("whisker.crateName", "whisker-local-store")
}

dependencies {
    // Phase J — single Whisker runtime dep. `:module-api` re-exports
    // `rs.whisker:annotations` transitively, so no separate dep on
    // the annotation JAR is needed. `ksp("rs.whisker:ksp")` stays
    // separate (it is a build-time processor, not on the runtime
    // classpath).
    implementation(project(":module-api"))
    ksp("rs.whisker:ksp")

    // Phase 7-Φ.G PoC — an external Maven dependency. AndroidX
    // Collection is small + pure-Kotlin (no native libs), so it
    // resolves quickly on cold caches. The import isn't used yet
    // (see WhiskerLocalStoreImpl.kt). The point of this entry is
    // to prove that module subprojects CAN declare arbitrary
    // Maven deps without any Whisker-side build plumbing.
    implementation("androidx.collection:collection:1.4.4")
}
