// `whisker-secure-store` ModuleDefinition (Android).
//
// A view-less DSL module: `definition()` has no `View(...)` block, just
// module-level `Function`s. The KSP processor finds the `Module`
// subclass and registers its functions with `WhiskerModuleRegistry`
// under the `Name(...)`, so `whisker::platform_module::invoke(
// "WhiskerSecureStore", ...)` from Rust routes into these handlers.
//
// Unlike `whisker-local-store`, the Tink / Keystore backend can throw
// (keyset corruption, decrypt failure after a credential reset, …), so
// each handler maps a caught `Throwable` to `WhiskerValue.Err(_)` —
// which the Rust wrapper lifts into `Err(WhiskerModuleError)`.
//
// The storage logic lives in `SecureStore.kt`.

package rs.whisker.modules.securestore

import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

class SecureStoreModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("WhiskerSecureStore")

        // save(key, value) -> Bool | Err
        Function("save") { args ->
            val key = args.getOrNull(0)?.asString() ?: ""
            val value = args.getOrNull(1)?.asString() ?: ""
            try {
                WhiskerValue.Bool(SecureStore.save(key, value))
            } catch (t: Throwable) {
                WhiskerValue.Err("WhiskerSecureStore.save failed: ${t.message}")
            }
        }
        // load(key) -> Str | Null | Err  (Rust lifts Null into Option::None)
        Function("load") { args ->
            try {
                SecureStore.load(args.getOrNull(0)?.asString() ?: "")
                    ?.let { WhiskerValue.Str(it) } ?: WhiskerValue.Null
            } catch (t: Throwable) {
                WhiskerValue.Err("WhiskerSecureStore.load failed: ${t.message}")
            }
        }
        // remove(key) -> Null | Err
        Function("remove") { args ->
            try {
                SecureStore.remove(args.getOrNull(0)?.asString() ?: "")
                WhiskerValue.Null
            } catch (t: Throwable) {
                WhiskerValue.Err("WhiskerSecureStore.remove failed: ${t.message}")
            }
        }
    }
}
