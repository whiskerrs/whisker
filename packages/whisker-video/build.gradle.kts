// Gradle build for `whisker-video`'s Android half (Phase 7-Φ.H.2.6).

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.video"
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
    arg("whisker.moduleName", "WhiskerVideo")
    arg("whisker.crateName", "whisker-video")
}

dependencies {
    implementation(project(":whisker-runtime"))
    implementation("rs.whisker:annotations")
    ksp("rs.whisker:ksp")
}
