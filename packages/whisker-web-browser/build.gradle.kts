plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.webbrowser"
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
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerWebBrowser")
    arg("whisker.crateName", "whisker-web-browser")
}

dependencies {
    // `0.1.7` — needs `WhiskerAppContext.DeepLinkListener`/
    // `addDeepLinkListener`/`removeDeepLinkListener`, added in this
    // same change and published by the `sdk-v0.1.7` release.
    implementation("rs.whisker:whisker-module-android:0.1.7")
    ksp("rs.whisker:ksp:0.1.7")
    implementation("androidx.browser:browser:1.8.0")
}
