package rs.whisker.gradle

import org.gradle.api.Plugin
import org.gradle.api.initialization.Settings
import java.io.File
import java.io.IOException
import java.security.MessageDigest

const val WHISKER_MODULE_REGISTRY_NAME = "whiskerModuleRegistry"

// Whisker's primary entry point on Android: a Settings plugin
// (id="rs.whisker") that the user declares once in
// `settings.gradle.kts`. Mirrors Expo's `expo-autolinking-settings`
// and Flutter's `dev.flutter.flutter-plugin-loader` — discover at
// Initialization phase, hand off to the per-project plugin via
// auto-apply + a BuildService.
//
// Usage (consumer-side):
//
// ```kotlin
// // settings.gradle.kts
// pluginManagement {
//     repositories {
//         maven { url = uri("https://whiskerrs.github.io/whisker/maven") }
//         google(); mavenCentral(); gradlePluginPortal()
//     }
// }
// plugins {
//     id("rs.whisker") version "0.1.0"
// }
// whisker {
//     workspace = file("../../..")
//     userPackage = "router-demo"
// }
// rootProject.name = "router-demo"
// include(":app")
// ```
//
// `app/build.gradle.kts` only declares AGP + Kotlin — no Whisker block.
// The Settings plugin auto-applies `rs.whisker.gradle` on every AGP
// project, which reads the module registry below and wires the rest.
class WhiskerPlugin : Plugin<Settings> {
    override fun apply(settings: Settings) {
        val ext = settings.extensions.create("whisker", WhiskerSettingsExtension::class.java)

        settings.gradle.settingsEvaluated {
            val workspace = ext.workspace.orNull?.asFile
                ?: error("rs.whisker: `whisker { workspace = file(...) }` is required.")
            val userPackage = ext.userPackage.orNull
                ?: error("rs.whisker: `whisker { userPackage = \"...\" }` is required.")

            val (json, report) = loadOrRefreshModulesReport(workspace, userPackage)

            // include() each Whisker module crate as a Gradle subproject.
            // Subproject path uses the crate name verbatim (`:whisker-router`)
            // so the host app can declare deps via the same string.
            report.modules.forEach { m ->
                m.android?.let { a ->
                    settings.include(":${m.crateName}")
                    settings.project(":${m.crateName}").projectDir = File(a.subprojectDir)
                }
            }

            // Register the BuildService so the Project plugin (and
            // anyone else who cares) can read the same module list
            // during Configuration phase.
            settings.gradle.sharedServices.registerIfAbsent(
                WHISKER_MODULE_REGISTRY_NAME,
                WhiskerModuleRegistry::class.java,
            ) {
                parameters.reportJson.set(json)
                parameters.workspace.set(workspace.absolutePath)
                parameters.userPackage.set(userPackage)
            }
        }

        // Auto-apply the project plugin on any project that ends up
        // with AGP applied. `withPlugin` fires whenever the plugin
        // becomes present, regardless of declaration order in the
        // user's build.gradle.kts. Mirrors expo's
        // `useExpoModules()` auto-config.
        //
        // Kotlin DSL ships `Gradle.beforeProject` as a
        // receiver-style lambda (`this` = Project). Capture the
        // outer `pluginManager` into `pm` so we can call `apply`
        // from inside `withPlugin { ... }` (whose own `this` is
        // `AppliedPlugin`, not `Project`).
        settings.gradle.beforeProject {
            val pm = pluginManager
            pm.withPlugin("com.android.application") {
                pm.apply(WhiskerProjectPlugin::class.java)
            }
            pm.withPlugin("com.android.library") {
                pm.apply(WhiskerProjectPlugin::class.java)
            }
        }
    }

    // Cache file: <workspace>/target/whisker/module-info.json. Sync
    // reuses it whenever Cargo.lock's hash matches; otherwise re-runs
    // `whisker-build modules`. Same idea as Flutter's
    // `.flutter-plugins-dependencies`.
    private fun loadOrRefreshModulesReport(
        workspace: File,
        userPackage: String,
    ): Pair<String, ModulesReport> {
        val cachePath = File(workspace, "target/whisker/module-info.json")
        val expectedHash = sha256OfCargoLock(workspace)

        if (cachePath.isFile && expectedHash != null) {
            try {
                val text = cachePath.readText()
                val cached = ModulesReport.parse(text)
                if (cached.cargoLockSha256 == expectedHash) {
                    return text to cached
                }
            } catch (_: Exception) {
                // Stale or corrupted cache — fall through to re-run.
            }
        }

        val text = runWhiskerBuildModules(workspace, userPackage)
        val parsed = ModulesReport.parse(text)
        try {
            cachePath.parentFile?.mkdirs()
            cachePath.writeText(text)
        } catch (_: IOException) {
            // Caching is a perf opt — failing to write the cache
            // doesn't break the build, just slows future Syncs.
        }
        return text to parsed
    }

    private fun sha256OfCargoLock(workspace: File): String? {
        val lock = File(workspace, "Cargo.lock")
        if (!lock.isFile) return null
        val md = MessageDigest.getInstance("SHA-256")
        val bytes = lock.readBytes()
        val hash = md.digest(bytes)
        return hash.joinToString("") { "%02x".format(it) }
    }

    private fun runWhiskerBuildModules(workspace: File, userPackage: String): String {
        val proc = ProcessBuilder(
            "whisker-build",
            "modules",
            "--workspace=${workspace.absolutePath}",
            "--package=$userPackage",
        ).redirectErrorStream(false).start()
        val out = proc.inputStream.bufferedReader().readText()
        val err = proc.errorStream.bufferedReader().readText()
        val rc = proc.waitFor()
        if (rc != 0) {
            error(
                "whisker-build modules failed (exit $rc).\n" +
                    "stderr:\n$err\n" +
                    "Hint: install with `cargo install whisker-build` " +
                    "and make sure it's on the JVM's PATH.",
            )
        }
        return out
    }
}
