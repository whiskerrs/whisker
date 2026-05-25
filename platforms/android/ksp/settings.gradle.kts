// Composite-build root for the Whisker Android KSP package.
//
// Two modules:
// - `annotations` — public `@WhiskerModule` Kotlin annotation, the
//   companion to iOS's `@WhiskerModule` Swift macro.
// - `ksp` — KSP processor that consumes `@WhiskerModule`
//   applications and generates `<Module>Behaviors.kt` in the
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

// `rootProject.name` MUST stay distinct from the inner `:ksp`
// submodule. `rs.whisker` is the inherited group; if both root and
// submodule publish artifact ID `ksp`, Gradle composite-build
// resolution errors with "Module version 'rs.whisker:ksp' is not
// unique in composite: can be provided by [project :ksp, project :ksp:ksp]".
rootProject.name = "whisker-android-ksp"

include(":annotations")
include(":ksp")
