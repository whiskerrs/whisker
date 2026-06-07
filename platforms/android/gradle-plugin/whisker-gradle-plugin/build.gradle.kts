// `whisker-gradle-plugin` — Project-scope plugin (id `rs.whisker.gradle`).
//
// Lives in its own JAR (NOT bundled with the Settings plugin
// `rs.whisker`). The split is mandatory: see the comment at the
// top of `whisker-settings-plugin/build.gradle.kts` for why.
//
// Reads its module list + workspace + userPackage from the state
// file the Settings plugin wrote at `<rootDir>/.whisker/config.properties`
// + the JSON at `<workspace>/target/whisker/module-info.json`.

plugins {
    `kotlin-dsl`
    `java-gradle-plugin`
    `maven-publish`
}

group = "rs.whisker"
version = "0.1.0"

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
        create("whiskerProject") {
            id = "rs.whisker.gradle"
            implementationClass = "rs.whisker.gradle.WhiskerProjectPlugin"
            displayName = "Whisker project plugin"
            description =
                "Per-AGP-module Whisker integration: aggregator Kotlin generation + cargo cross-compile + jniLibs staging."
        }
    }
}

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
