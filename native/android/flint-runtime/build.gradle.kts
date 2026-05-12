plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.flint.runtime"
    compileSdk = 34

    defaultConfig {
        minSdk = 24

        externalNativeBuild {
            cmake {
                arguments += listOf("-DANDROID_STL=c++_shared")
                cppFlags += "-std=c++17"
            }
        }

        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
        }
    }
}

dependencies {
    // Lynx prebuilt (3.7.0 stable as of 2026-05).
    implementation("org.lynxsdk.lynx:lynx:3.7.0")
    implementation("org.lynxsdk.lynx:lynx-jssdk:3.7.0")
    implementation("org.lynxsdk.lynx:primjs:3.7.0")

    implementation("androidx.appcompat:appcompat:1.7.0")
}
