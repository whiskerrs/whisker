// `whisker-settings-plugin` — Settings-scope plugin (id `rs.whisker`).
//
// Lives in its own JAR (NOT the same JAR as the project plugin) so
// it can load into the settings classloader without dragging
// project-plugin classes that reference AGP. The two-JAR split is
// the only working answer: the Settings plugin's classloader has
// no AGP visible, so any settings-side reference to
// `AndroidComponentsExtension` (which `WhiskerProjectPlugin` does
// have) throws `NoClassDefFoundError` on apply.
//
// Cross-plugin data hand-off is via a state file on disk:
//   * Settings plugin writes `<workspace>/target/whisker/module-info.json`
//     (the `whisker-build modules` raw output)
//   * Settings plugin writes `<rootDir>/.whisker/config.properties`
//     (workspace + userPackage echo so the Project plugin can find
//     the workspace without a second `whisker { ... }` block)
//   * Project plugin reads both at apply() time.

plugins {
    `kotlin-dsl`
    `java-gradle-plugin`
    `maven-publish`
}

group = "rs.whisker"
version = "0.1.0"

gradlePlugin {
    plugins {
        create("whiskerSettings") {
            id = "rs.whisker"
            implementationClass = "rs.whisker.gradle.WhiskerPlugin"
            displayName = "Whisker Settings plugin"
            description =
                "Discovers Whisker module deps via `whisker-build modules`, includes each as a Gradle subproject, and stages a state file the Project plugin (`rs.whisker.gradle`) reads."
        }
    }
}

// Same publish setup as whisker-gradle-plugin — CI overrides via
// `-PpublishUrl=file://...maven-out` so the gh-pages workflow
// targets a workspace-local dir before peaceiris/actions-gh-pages
// pushes it.
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
