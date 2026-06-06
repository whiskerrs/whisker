// Standalone build root for the Whisker Gradle plugin so it can be
// developed (and `./gradlew publishToMavenLocal`-ed) without dragging
// the consuming app's settings + repository pins in. Published as
// `rs.whisker:whisker-gradle-plugin` to the same gh-pages Maven repo
// that hosts the Lynx fork AARs — apps declare it as
// `id("rs.whisker.gradle") version "<ver>"` in their settings.

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

rootProject.name = "whisker-gradle-plugin"
