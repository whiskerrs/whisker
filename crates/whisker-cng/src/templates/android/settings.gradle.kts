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

rootProject.name = "{{android_project_name}}"
include(":app")
include(":whisker-runtime")
project(":whisker-runtime").projectDir = file("{{whisker_runtime_android_path}}")
