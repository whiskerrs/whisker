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
        // Lynx AARs land here from `cargo xtask android build-lynx-aar`.
        // Declared at settings-level (not whisker-runtime-level) so the
        // strict FAIL_ON_PROJECT_REPOS mode above doesn't reject it,
        // and so the consuming-app's settings is the single source of
        // truth for "where do Lynx artifacts come from".
        flatDir {
            dirs("{{whisker_lynx_aar_dir}}")
        }
    }
}

rootProject.name = "{{android_project_name}}"
include(":app")
include(":whisker-runtime")
project(":whisker-runtime").projectDir = file("{{whisker_runtime_android_path}}")
