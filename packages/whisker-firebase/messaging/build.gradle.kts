plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.3.11"
}

android {
    namespace = "rs.whisker.modules.firebasemessaging"
    compileSdk = 35
    defaultConfig { minSdk = 23 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    sourceSets.getByName("main") {
        kotlin.srcDirs("android/src/main/kotlin")
        manifest.srcFile("android/src/main/AndroidManifest.xml")
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerFirebaseMessaging")
    arg("whisker.crateName", "whisker-firebase-messaging")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.21")
    ksp("rs.whisker:ksp:0.1.21")
    implementation(platform("com.google.firebase:firebase-bom:34.19.0"))
    implementation("com.google.firebase:firebase-messaging")
    implementation("androidx.activity:activity:1.8.2")
}
