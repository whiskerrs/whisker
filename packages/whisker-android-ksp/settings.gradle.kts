// Composite-build root for the Whisker Android KSP package.
//
// Two modules:
// - `annotations` — public `@WhiskerElement` Kotlin annotation, the
//   companion to iOS's `@WhiskerElement` Swift Macro (Phase H.1).
// - `ksp` — KSP processor that consumes `@WhiskerElement`
//   applications and generates `WhiskerModuleBehaviors.kt` in the
//   user app's source set. Replaces the manual whisker-build-time
//   Kotlin generation from Phase 7-Φ.C.
//
// Consumed by the generated user app via `includeBuild("...")` in
// `gen/android/settings.gradle.kts` — composite-build dep, no Maven
// publish required.

pluginManagement {
    repositories {
        gradlePluginPortal()
        google()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.PREFER_PROJECT)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "whisker-android-ksp"

include(":annotations")
include(":ksp")
