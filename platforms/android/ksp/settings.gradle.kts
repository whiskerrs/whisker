// Composite-build root for the Whisker Android KSP package.
//
// Single module:
// - `ksp` — KSP processor that scans the user app's compilation
//   for `rs.whisker.runtime.Module` subclasses and generates
//   `<Module>Behaviors.kt`. Discovery is inheritance-based — Phase
//   M (Issue #59) dropped the `@WhiskerModule` marker annotation
//   that previously gated registration, so the `annotations`
//   subproject is gone.
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

include(":ksp")
