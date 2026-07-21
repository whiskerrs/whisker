// Resolves the app's per-directory sandbox paths from the Android
// Context.
//
// Plain helper — no Whisker / Lynx types. The DSL module that exposes
// it to Rust lives in `PathsModule.kt`. Returns plain filesystem paths
// so the Rust side can use them with std::fs directly.
//
// Android has no distinct "documents" / "application-support" / "temp"
// dirs the way iOS does, so we derive them from the two real internal
// roots: cacheDir (evictable) and filesDir (persistent). `support` and
// `temp` are subdirectories; a returned path may not exist yet, so the
// caller creates it with std::fs::create_dir_all.

package rs.whisker.modules.paths

import android.content.Context
import rs.whisker.runtime.WhiskerApplication
import java.io.File

internal object Paths {
    /**
     * Resolve the app context, installed by the host app's
     * `Application.onCreate()` before any module dispatch reaches here.
     */
    private fun context(): Context =
        WhiskerApplication.appContext
            ?: throw IllegalStateException(
                "WhiskerPaths: WhiskerApplication.appContext not initialised — " +
                    "ensure your Application extends WhiskerApplication and " +
                    "super.onCreate() runs before any module dispatch",
            )

    /// The four per-app directories, keyed to match the Rust side.
    fun directories(): Map<String, String> {
        val ctx = context()
        return mapOf(
            "cache" to ctx.cacheDir.absolutePath,
            "document" to ctx.filesDir.absolutePath,
            "support" to File(ctx.filesDir, "support").absolutePath,
            "temp" to File(ctx.cacheDir, "tmp").absolutePath,
        )
    }
}
