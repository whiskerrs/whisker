plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.surface"
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
    arg("whisker.moduleName", "WhiskerSurface")
    arg("whisker.crateName", "whisker-surface")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.21")
    ksp("rs.whisker:ksp:0.1.21")

    testImplementation("junit:junit:4.13.2")
}
