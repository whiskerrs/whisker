pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "whisker-host-conformance-android"
include(":app", ":whisker-module", ":whisker-runtime")
project(":whisker-module").projectDir = file("../../../../platforms/android/module")
project(":whisker-runtime").projectDir = file("../../../../platforms/android/runtime")
