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
include(":module-api")
project(":module-api").projectDir = file("{{whisker_module_api_android_path}}")
include(":whisker-runtime")
project(":whisker-runtime").projectDir = file("{{whisker_runtime_android_path}}")

// Phase 7-Φ.H.2: `ksp` brings the `@WhiskerComponent`
// annotation + KSP processor into the build via Gradle's
// composite-build mechanism. The included build resolves
// `rs.whisker:annotations` and `rs.whisker:ksp` against its own
// subprojects (see `platforms/android/ksp/settings.gradle.kts`),
// so the app's `build.gradle.kts` references them by group:artifact
// like any regular external dep.
//
// Composite builds run in their own daemon-internal classloader, so
// pinning the included build's Kotlin version to match the consuming
// app's (2.0.21) is what keeps the KSP processor's symbols ABI-
// compatible with the user app's Kotlin compiler.
includeBuild("{{whisker_android_ksp_path}}")

// Phase 7-Φ.G: Whisker module packages are now Gradle subprojects
// of the user app. The actual `include(":<crate-name>")` +
// `project(...).projectDir = file(...)` calls are emitted by
// whisker-build into `whisker_modules.settings.gradle.kts` (next
// to this file) so the list refreshes automatically when a
// cargo dep is added / removed.
//
// `apply(from = ...)` invokes the external script with this
// settings instance as the receiver, so its top-level `include`
// calls register subprojects on the user app's Gradle build.
val whiskerModulesSettings = file("whisker_modules.settings.gradle.kts")
if (whiskerModulesSettings.exists()) {
    apply(from = whiskerModulesSettings)
}
