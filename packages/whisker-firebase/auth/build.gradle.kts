plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("com.google.devtools.ksp") version "2.3.12"
}

android {
    namespace = "rs.whisker.modules.firebaseauth"
    compileSdk = 35
    defaultConfig { minSdk = 23 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    sourceSets.getByName("main").kotlin.srcDirs("android/src/main/kotlin")
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        // firebase-auth 24.x ships Kotlin 2.3 metadata; Whisker apps compile with Kotlin 2.0.
        freeCompilerArgs.add("-Xskip-metadata-version-check")
    }
}

ksp {
    arg("whisker.moduleName", "WhiskerFirebaseAuth")
    arg("whisker.crateName", "whisker-firebase-auth")
}

dependencies {
    implementation("rs.whisker:whisker-module-android:0.1.21")
    ksp("rs.whisker:ksp:0.1.21")
    implementation(platform("com.google.firebase:firebase-bom:34.19.0"))
    implementation("com.google.firebase:firebase-auth")
}
