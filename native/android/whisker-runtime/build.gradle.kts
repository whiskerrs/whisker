plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "rs.whisker.runtime"
    compileSdk = 34

    defaultConfig {
        minSdk = 24

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
}

// Local AARs produced by scripts/build-lynx-android.sh (which patches the
// upstream Lynx build so it exports the Element PAPI symbols our Rust
// bridge calls into). PrimJS itself can stay on Maven — we don't depend
// on its private symbols.
//
// `projectDir` is `native/android/whisker-runtime/`; the whisker repo root is
// three levels up. Resolving here (rather than via rootProject) keeps
// the path correct whether this module is built standalone or pulled
// into an example app via `include(":whisker-runtime")`.
val lynxAarDir = projectDir.resolve("../../../target/lynx-android")

dependencies {
    // `api` (not `implementation`) so consuming apps can see LynxView /
    // LynxEnv types that leak through `WhiskerView`'s superclass.
    api(files("$lynxAarDir/LynxAndroid.aar"))
    api(files("$lynxAarDir/LynxBase.aar"))
    api(files("$lynxAarDir/LynxTrace.aar"))
    api(files("$lynxAarDir/ServiceAPI.aar"))
    api("org.lynxsdk.lynx:primjs:3.7.0")

    implementation("androidx.appcompat:appcompat:1.7.0")
}
