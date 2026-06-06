// Multi-project build root for the Whisker Gradle plugins. Hosts two
// separate plugin JARs that consumers resolve independently:
//
//   * `rs.whisker:whisker-settings-plugin` — Settings-scope plugin
//     (id `rs.whisker`). User declares once in settings.gradle.kts.
//
//   * `rs.whisker:whisker-gradle-plugin` — Project-scope plugin
//     (id `rs.whisker.gradle`). User declares in app/build.gradle.kts.
//
// They MUST live in separate JARs because the Settings plugin loads
// into the settings classloader (which has no AGP visible), and
// reusing the same JAR for the Project plugin inherits that
// classloader — causing `AndroidComponentsExtension` lookups in
// the Project plugin's `apply()` to throw `NoClassDefFoundError`.

pluginManagement {
    repositories {
        gradlePluginPortal()
        google()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

rootProject.name = "whisker-gradle"
include(":whisker-settings-plugin")
include(":whisker-gradle-plugin")
