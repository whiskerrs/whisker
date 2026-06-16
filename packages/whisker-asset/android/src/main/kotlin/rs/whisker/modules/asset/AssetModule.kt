// `whisker-asset` Module (Android). View-less, declaration-only.
//
// ## Where the asset base actually gets installed
//
// The Android resolver base is the fixed constant
// `file:///android_asset/whisker/` — there is no runtime data to ferry
// across the Kotlin↔Rust boundary (contrast iOS, whose bundle path is
// only known at runtime and so MUST come from native Swift). Installing
// it therefore happens entirely on the Rust side: `whisker-asset`'s lib
// registers an `.init_array` constructor (`install_android_base`) that
// runs the instant the app's `.so` is loaded via
// `System.loadLibrary` — well before the first render or any
// `asset!(…)` → `whisker_asset::resolve(…)` call. See
// `packages/whisker-asset/src/lib.rs`.
//
// Calling the `whisker_asset_set_android()` C export from Kotlin would
// require a JNI C++ shim (Kotlin `external fun` resolves only
// `Java_*`-named symbols, not bare C symbols), and that shim lives in
// the driver bridge crate, not here — so the Rust constructor is the
// cleaner, self-contained path. The C export remains available for a
// future host that wants to flip the base explicitly.
//
// This Kotlin module exists so the `[package.metadata.whisker]` marker
// wires a real Android subproject (KSP registration, parity with the
// iOS `AssetModule`) and as the home for any future native asset work.
// Its `definition()` only declares the module name.

package rs.whisker.modules.asset

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition

public class AssetModule : Module() {
    public override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("WhiskerAsset")
    }
}
