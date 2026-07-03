// `whisker-module` — minimal Android surface for third-party
// Whisker modules. Carries `WhiskerValue`, `WhiskerUI` /
// `WhiskerContext` typealiases, `WhiskerApplication`. Separate from
// `:whisker-runtime` so modules pull in just the types they need
// (no host-side `WhiskerActivity` / `WhiskerView` /
// `WhiskerModuleRegistry`).
//
// Published as `rs.whisker:whisker-module-android`. The `lynxFork`
// / `whiskerSdkRelease` Gradle property toggle works the same way
// as in `:whisker-runtime` — see that file's header comment.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    `maven-publish`
}

group = "rs.whisker"
version = "0.0.0-dev"

android {
    // AGP namespace MUST be distinct from `:whisker-runtime`'s
    // (which also lives under the `rs.whisker.runtime` Kotlin
    // package) — otherwise AGP errors out with "Namespace is used
    // in multiple modules". The Kotlin sources inside can still
    // declare `package rs.whisker.runtime` for stable external
    // imports; this AGP-level namespace just disambiguates the
    // R class.
    namespace = "rs.whisker.runtime.moduleapi"
    compileSdk = 34

    defaultConfig {
        minSdk = 24

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

    publishing {
        singleVariant("release") {
            withSourcesJar()
            withJavadocJar()
        }
    }
}

val whiskerSdkRelease = providers.gradleProperty("whiskerSdkRelease").orNull == "true"
val lynxFork = providers.gradleProperty("lynxFork").getOrElse("v3.8.0-whisker.10").removePrefix("v")

dependencies {
    if (whiskerSdkRelease) {
        api("rs.whisker:lynx-android:$lynxFork")
        api("rs.whisker:lynx-base-android:$lynxFork")
        api("rs.whisker:lynx-trace-android:$lynxFork")
        api("rs.whisker:lynx-service-api-android:$lynxFork")
    } else {
        api(":LynxAndroid@aar")
        api(":LynxBase@aar")
        api(":LynxTrace@aar")
        api(":ServiceAPI@aar")
    }
    api("org.lynxsdk.lynx:primjs:3.7.0")

    // No annotation re-export needed (Phase M / Issue #59): a
    // module's `build.gradle.kts` depends on this AAR alone for
    // runtime types; the `ksp(...)` processor stays separate as a
    // build-time dep.
}

publishing {
    publications {
        register<MavenPublication>("release") {
            afterEvaluate {
                from(components["release"])
            }
            artifactId = "whisker-module-android"
            pom {
                name.set("Whisker module API (Android)")
                description.set(
                    "Minimal Android surface for third-party Whisker modules — " +
                        "WhiskerValue, Whisker* typealiases, WhiskerApplication.",
                )
                url.set("https://github.com/whiskerrs/whisker")
                licenses {
                    license {
                        name.set("MIT")
                        url.set("https://github.com/whiskerrs/whisker/blob/main/LICENSE")
                    }
                }
            }
        }
    }
    repositories {
        maven {
            name = "ghPages"
            url = uri(providers.gradleProperty("publishUrl").orElse("file://${rootProject.layout.buildDirectory.get()}/repo").get())
        }
    }
}
