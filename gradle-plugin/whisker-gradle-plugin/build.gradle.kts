// `whisker-gradle-plugin` — the Android side of the build-system
// migration. Applied at the consuming app's `app/build.gradle.kts`
// via `id("rs.whisker.gradle")` after AGP. The plugin:
//
//   1. Reads a `whisker { ... }` extension that names the user's
//      Cargo workspace root + package name.
//   2. Registers a per-variant per-ABI `whiskerBuild<Variant><Abi>`
//      task that shells out to the `whisker-build` binary
//      (PATH-resolved, built by `cargo install whisker-build`) with
//      the workspace / package / profile / abi / jni-libs-dir /
//      min-sdk inputs Gradle's variant model carries.
//   3. Wires the resulting `.so` (and any module-system gradle
//      subprojects whisker-build emits) into the Android variant's
//      jniLibs so the standard `assemble<Variant>` produces an APK
//      that loads the Rust dylib.
//
// `cargo` cross-compilation logic itself lives in `whisker-build` —
// the plugin is purely the AGP-integration shim.

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
        create("whisker") {
            id = "rs.whisker.gradle"
            implementationClass = "rs.whisker.gradle.WhiskerPlugin"
            displayName = "Whisker Gradle plugin"
            description =
                "Cross-compiles Whisker (Rust) modules for Android and stages the resulting .so into the AGP variant's jniLibs."
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
