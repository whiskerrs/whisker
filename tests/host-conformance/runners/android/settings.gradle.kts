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
include(":app", ":whisker-module")
project(":whisker-module").projectDir = file("../../../../platforms/android/module")
