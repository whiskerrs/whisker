// `whisker-gradle-plugin` — the Android side of the build-system
// migration. Ships TWO plugin IDs from the same JAR:
//
//   * `rs.whisker` (Settings plugin, [WhiskerPlugin]) — applied once
//     in `settings.gradle.kts`. Spawns `whisker-build modules` to
//     discover Whisker module deps at Initialization phase,
//     `include()`s each one, registers a BuildService, and
//     auto-applies the project-scope plugin on every AGP project
//     via `gradle.beforeProject`.
//
//   * `rs.whisker.gradle` (Project plugin, [WhiskerProjectPlugin])
//     — registers a per-variant aggregator Kotlin generator and
//     per-variant per-ABI cargo cross-compile task. Normally
//     auto-applied; the standalone ID stays available for
//     opt-in scenarios.
//
// `cargo` cross-compilation logic itself lives in `whisker-build` —
// the plugins are purely the Gradle-integration shims.

plugins {
    `kotlin-dsl`
    `java-gradle-plugin`
    `maven-publish`
}

group = "rs.whisker"
// Kept in lockstep with the consuming whisker crate's version so the
// `id("rs.whisker.gradle") version "<ver>"` line in a user app's
// settings.gradle.kts reads consistently with the rest of their
// Whisker dependencies.
version = "0.1.0"

// Repositories are declared once in `settings.gradle.kts` under
// `dependencyResolutionManagement` (FAIL_ON_PROJECT_REPOS) — don't
// re-declare them here.

dependencies {
    // AGP's `AndroidComponentsExtension` — the modern (8.0+) variant
    // API the plugin reaches into to register the per-variant cargo
    // tasks. `compileOnly` because the consuming project always
    // brings its own AGP version and we don't want to drag a second
    // copy into the classpath.
    compileOnly("com.android.tools.build:gradle:8.6.1")
}

gradlePlugin {
    plugins {
        // Settings-scope entry point. Users declare this once in
        // `settings.gradle.kts`; it runs `whisker-build modules` to
        // discover module subprojects, `include()`s each one,
        // registers the `WhiskerModuleRegistry` BuildService, and
        // auto-applies the project-scope plugin onto every AGP
        // module via `gradle.beforeProject`. Mirrors Expo's
        // `expo-autolinking-settings` / Flutter's
        // `dev.flutter.flutter-plugin-loader`.
        create("whiskerSettings") {
            id = "rs.whisker"
            implementationClass = "rs.whisker.gradle.WhiskerPlugin"
            displayName = "Whisker Settings plugin"
            description =
                "Discovers Whisker module deps and wires them into the consuming AGP project."
        }

        // Project-scope plugin. Auto-applied by the Settings plugin
        // above. The standalone ID is kept for explicit-opt-in
        // scenarios (multi-build composites, classloader quirks)
        // and so its behaviour is testable in isolation.
        create("whiskerProject") {
            id = "rs.whisker.gradle"
            implementationClass = "rs.whisker.gradle.WhiskerProjectPlugin"
            displayName = "Whisker project plugin"
            description =
                "Per-AGP-module Whisker integration: aggregator Kotlin generation + cargo cross-compile + jniLibs staging."
        }
    }
}

// Standard `maven-publish` setup — the CI step that publishes the
// plugin to gh-pages overrides `publishing.repositories.maven.url`
// via `-PpublishUrl=...`. Locally `./gradlew publishToMavenLocal`
// just resolves against `~/.m2`.
publishing {
    repositories {
        maven {
            name = "ghPages"
            url = uri(providers.gradleProperty("publishUrl").orElse("file://${rootProject.layout.buildDirectory.get()}/repo").get())
        }
    }
}

kotlin {
    jvmToolchain(17)
}
