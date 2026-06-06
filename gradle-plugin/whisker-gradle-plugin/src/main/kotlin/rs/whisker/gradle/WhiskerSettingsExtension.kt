package rs.whisker.gradle

import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property

// The `whisker { ... }` block users put in `settings.gradle.kts` to
// configure the Settings plugin. Carries:
//
//   * `workspace` — cargo workspace root containing the user app's
//     top-level `Cargo.toml`. The Settings plugin spawns
//     `whisker-build modules --workspace=<this>` to discover deps.
//
//   * `userPackage` — the user app's cargo crate name. Walks the dep
//     graph rooted here, picks every dep with
//     `[package.metadata.whisker]`.
//
// Both are required. The Project plugin re-reads the same values from
// the Settings extension via the `WhiskerModuleRegistry` BuildService
// so users don't have to declare a second `whisker { ... }` block in
// `app/build.gradle.kts`.
abstract class WhiskerSettingsExtension {
    abstract val workspace: DirectoryProperty

    // Underscore suffix because plain `package` is a Kotlin soft
    // keyword (legal in DSL but reads odd in IDE completion).
    abstract val userPackage: Property<String>
}
