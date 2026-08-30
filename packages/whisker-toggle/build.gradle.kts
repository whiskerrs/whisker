plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.toggle"
    compileSdk = 34

    defaultConfig { minSdk = 21 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    sourceSets {
        getByName("main") { kotlin.srcDirs("android/src/main/kotlin") }
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerToggle")
    arg("whisker.crateName", "whisker-toggle")
}

dependencies {
    if (rootProject.findProject(":whisker-module") != null) {
        implementation(project(":whisker-module"))
    } else {
        implementation("rs.whisker:whisker-module-android:0.1.20")
    }
    ksp("rs.whisker:ksp:0.1.20")
}
