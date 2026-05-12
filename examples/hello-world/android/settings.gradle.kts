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
    }
}

rootProject.name = "hello-world-android"
include(":app")
include(":lyra-runtime")
project(":lyra-runtime").projectDir = file("../../../native/android/lyra-runtime")
