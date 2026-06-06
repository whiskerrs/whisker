// Gradle build for `whisker-svg`'s Android half. Mirrors
// `whisker-image` / `whisker-safe-area`'s shape: one KSP-processed
// module subproject with sources under `android/src/main/kotlin`,
// pure-Kotlin (no native deps). The replayer paints directly into
// the View's own `Canvas` via `onDraw`, so AndroidSVG / Serval /
// Skia aren't needed — the entire SVG → pixels path goes through
// the Rust producer + this thin Kotlin replayer.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.svg"
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
            kotlin.srcDirs("android/src/main/kotlin")
        }
        getByName("test") {
            kotlin.srcDirs("android/src/test/kotlin")
        }
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerSvg")
    arg("whisker.crateName", "whisker-svg")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.0")
    ksp("rs.whisker:ksp:0.1.0")

    testImplementation("junit:junit:4.13.2")
}
