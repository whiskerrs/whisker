// Gradle build for `whisker-webview`'s Android half.
//
// Mirrors `whisker-input`'s shape: one KSP-processed module subproject
// with sources under `android/src/main/kotlin`, depending on Whisker's
// runtime. The android.webkit.WebView we wrap is part of the Android
// framework (minSdk 21 supports it fully). The androidx.webkit library
// is added for WebViewCompat.addDocumentStartJavaScript — the preferred
// document-start injection path (avoids the onPageStarted race). If the
// compiled device API doesn't have the underlying method, the compat
// library gracefully falls back (min API 24 for addDocumentStartJavaScript;
// on API 21–23 we fall back to onPageStarted injection in the view).

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.webview"
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
    arg("whisker.moduleName", "WhiskerWebview")
    arg("whisker.crateName", "whisker-webview")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.0")
    ksp("rs.whisker:ksp:0.1.0")

    // AndroidX WebKit compat — provides WebViewCompat.addDocumentStartJavaScript
    // (API 24+) for reliable document-start JS injection without a race against
    // onPageStarted. The compat library is ~60 KB; no other dep pulled in.
    implementation("androidx.webkit:webkit:1.12.1")
}
