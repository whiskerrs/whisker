package rs.whisker.gradle

import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property

/// The `whisker { ... }` DSL block users configure in
/// `app/build.gradle.kts`. Drives every value the plugin needs to
/// hand `whisker-build android` per Gradle variant.
///
/// ```kotlin
/// whisker {
///     workspace.set(file("../../.."))         // cargo workspace root
///     package_.set("my-app")                   // user crate name
///     abis.set(listOf("arm64-v8a"))            // ABIs to cross-compile
///     // minSdk is read from the AGP DSL by default;
///     // override here only for unusual cases.
/// }
/// ```
abstract class WhiskerExtension {
    /// Absolute path to the cargo workspace containing the user
    /// Whisker app crate. Resolves the `Cargo.toml` the plugin's
    /// per-variant cargo tasks point at. Typically a few `..` up
    /// from the AGP project root.
    abstract val workspace: DirectoryProperty

    /// User Whisker crate name (`name` in the user crate's
    /// `Cargo.toml`). Passed verbatim to `whisker-build android
    /// --package`; matches what `cargo metadata` reports.
    ///
    /// Suffixed `_` because plain `package` is a Kotlin soft keyword
    /// for source-file declarations.
    abstract val package_: Property<String>

    /// ABIs to cross-compile per variant. Each ABI fans out to one
    /// `cargo rustc --target=<triple>` invocation inside
    /// `whisker-build android`. Defaults to `[arm64-v8a]` — the
    /// modern-Android single-arch case Whisker's `whisker-driver-sys`
    /// already supports. Add others (`armeabi-v7a`, `x86_64`, `x86`)
    /// when bringing up multi-arch support.
    abstract val abis: org.gradle.api.provider.ListProperty<String>

    /// Override the Android API level handed to the NDK toolchain.
    /// When unset the plugin reads the AGP DSL's `minSdk` so the
    /// Rust dylib targets the same sysroot AGP's Kotlin/Java
    /// compile uses.
    abstract val minSdk: Property<Int>
}
