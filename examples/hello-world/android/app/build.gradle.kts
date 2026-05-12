plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.lyra.examples.helloworld"
    compileSdk = 34
    ndkVersion = "21.1.6352462"

    defaultConfig {
        applicationId = "dev.lyra.examples.helloworld"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"

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

    // The Rust cdylib (libhello_world.so) is compiled by
    // scripts/build-android-example.sh and dropped into this dir.
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
                jniLibs.keepDebugSymbols += listOf("**/libhello_world.so")
            }
        }
    }
}

dependencies {
    implementation(project(":lyra-runtime"))
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.core:core-ktx:1.13.1")
}
