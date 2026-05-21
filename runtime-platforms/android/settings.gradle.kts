// Phase 7-A.3 skeleton. Standalone gradle project — included via
// `includeBuild` from the user app's `gen/android/settings.gradle.kts`
// when autolink (Phase 7-D) materialises it.

rootProject.name = "whisker-native-runtime-android"

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}
