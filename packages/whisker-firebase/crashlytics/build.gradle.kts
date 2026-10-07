plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.0.21-1.0.27"
}

android {
    namespace = "rs.whisker.modules.firebasecrashlytics"
    compileSdk = 35
    defaultConfig { minSdk = 23 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    sourceSets.getByName("main").kotlin.srcDirs("android/src/main/kotlin")
}

ksp {
    arg("whisker.moduleName", "WhiskerFirebaseCrashlytics")
    arg("whisker.crateName", "whisker-firebase-crashlytics")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.21")
    ksp("rs.whisker:ksp:0.1.21")
    implementation(platform("com.google.firebase:firebase-bom:34.19.0"))
    implementation("com.google.firebase:firebase-crashlytics")
    // Rust panics abort natively; only the NDK library reports native crashes.
    implementation("com.google.firebase:firebase-crashlytics-ndk")
}
