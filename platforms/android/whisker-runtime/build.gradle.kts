// `whisker-runtime` — the Whisker Android host-side runtime AAR.
// Carries `WhiskerActivity`, `WhiskerView`, `WhiskerModuleRegistry`,
// and the Kotlin half of the reactive runtime. Consumed by the user
// app via Maven coordinate `rs.whisker:whisker-runtime-android` once
// the gh-pages Maven repo (#145) carries it.
//
// Two consumption modes coexist on a single source tree:
//
//   1. **Legacy local-aar flow** — historical builds registered
//      `target/lynx-android/` as a `flatDir` and resolved
//      `:LynxAndroid@aar` style refs through it. Modern `whisker run`
//      lets gradle pull the same artefacts from the Maven repo (mode
//      2 below) instead; the flatDir entry remains harmless when
//      empty.
//
//   2. **Maven-driven flow** (Step 5-Android target) — the user
//      app pulls this AAR by Maven coord, and its transitive deps
//      need real Maven coordinates so Gradle can resolve them.
//
// `whiskerSdkRelease` Gradle property toggles the dep form: unset
// (default) → flatDir-friendly `:LynxAndroid@aar`; `true` → Maven
// coords pinned to `lynxFork`. The CI publish workflow sets
// `-PwhiskerSdkRelease=true -PlynxFork=v3.8.0-whisker.6`; local CLI
// flows leave both unset and use flatDir as before.

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    `maven-publish`
}

// Group + version are picked up by the maven-publish block below.
// Version is sed-stamped by `.github/workflows/publish-sdk.yml` from
// the `sdk-v*` tag the workflow fires on; `0.0.0-dev` is the
// `workflow_dispatch` default so a manual smoke run produces an
// obvious-not-real version.
group = "rs.whisker"
version = "0.0.0-dev"

android {
    namespace = "rs.whisker.runtime"
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

    // Tell maven-publish to pick up the `release` variant's AAR.
    // Without this, AGP's default-variant guessing produces an
    // ambiguous component and the publish task fails.
    publishing {
        singleVariant("release") {
            withSourcesJar()
            withJavadocJar()
        }
    }
}

val whiskerSdkRelease = providers.gradleProperty("whiskerSdkRelease").orNull == "true"
val lynxFork = providers.gradleProperty("lynxFork").getOrElse("v3.8.0-whisker.6").removePrefix("v")

dependencies {
    api(project(":module"))

    if (whiskerSdkRelease) {
        // Maven-driven: pin Lynx AARs to the fork's gh-pages Maven
        // (`whiskerrs.github.io/lynx/maven`). The consuming app's
        // settings.gradle.kts must list that repo in
        // `dependencyResolutionManagement` — the `rs.whisker.gradle`
        // plugin's smoke + Step-5 cng template both do.
        api("rs.whisker:lynx-android:$lynxFork")
        api("rs.whisker:lynx-base-android:$lynxFork")
        api("rs.whisker:lynx-trace-android:$lynxFork")
        api("rs.whisker:lynx-service-api-android:$lynxFork")
    } else {
        // Local-CLI / dev path — flatDir registers `LynxAndroid.aar`
        // etc. under `target/lynx-android/`. The `:<name>@aar`
        // Kotlin-DSL form is the AGP-blessed way to reference an
        // AAR with no group.
        api(":LynxAndroid@aar")
        api(":LynxBase@aar")
        api(":LynxTrace@aar")
        api(":ServiceAPI@aar")
    }
    api("org.lynxsdk.lynx:primjs:3.7.0")

    implementation("androidx.appcompat:appcompat:1.7.0")
}

publishing {
    publications {
        register<MavenPublication>("release") {
            // Wait until `afterEvaluate` because the AGP `release`
            // component the `from(...)` call below references is
            // only materialised after AGP's variant model has run.
            afterEvaluate {
                from(components["release"])
            }
            artifactId = "whisker-runtime-android"
            pom {
                name.set("Whisker runtime (Android)")
                description.set(
                    "Host-side Whisker Android runtime — WhiskerActivity, " +
                        "WhiskerView, WhiskerModuleRegistry, reactive runtime Kotlin half.",
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
