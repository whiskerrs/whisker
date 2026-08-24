plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val hostConformanceAssets = layout.buildDirectory.dir("generated/hostConformanceAssets")
val stageHostConformanceAssets by tasks.registering(Sync::class) {
    from(file("../../..")) {
        include("manifest.json", "core/**", "wpt/**")
    }
    into(hostConformanceAssets)
}

android {
    namespace = "rs.whisker.conformance"
    compileSdk = 34

    defaultConfig {
        applicationId = "rs.whisker.conformance"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "1"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    sourceSets.getByName("androidTest").assets.srcDir(hostConformanceAssets)
}

tasks.configureEach {
    if (name == "mergeDebugAndroidTestAssets") dependsOn(stageHostConformanceAssets)
}

dependencies {
    implementation(project(":whisker-runtime"))
    androidTestImplementation("androidx.test:core-ktx:1.6.1")
    androidTestImplementation("androidx.test.ext:junit-ktx:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
}
