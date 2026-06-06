package rs.whisker.gradle

import org.gradle.api.Plugin
import org.gradle.api.initialization.Settings
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.Properties

// Settings-scope entry point. Users declare this in
// `settings.gradle.kts`:
//
// ```kotlin
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
// On apply this plugin:
//
//   1. Spawns `whisker-build modules --workspace=... --package=...`
//      and writes the JSON output to
//      `<workspace>/target/whisker/module-info.json`. Cached by
//      Cargo.lock SHA-256 so a warm Sync skips cargo metadata
//      (mirrors Flutter's `.flutter-plugins-dependencies` model).
//   2. Reads the JSON, `include()`s each Whisker module as a Gradle
//      subproject, sets each one's `projectDir` to the module's
//      package root.
//   3. Writes `<rootDir>/.whisker/config.properties` with
//      `workspace=<abs>` + `user_package=<name>` so the
//      Project plugin (`rs.whisker.gradle`, applied separately by
//      the user in `app/build.gradle.kts`) can find the same
//      values without a second `whisker { ... }` block.
//
// The Project plugin must live in a SEPARATE JAR — see the comment
// at the top of `whisker-settings-plugin/build.gradle.kts` for the
// classloader-isolation rationale.
class WhiskerPlugin : Plugin<Settings> {
    override fun apply(settings: Settings) {
        val ext = settings.extensions.create("whisker", WhiskerSettingsExtension::class.java)

        settings.gradle.settingsEvaluated {
            val workspace = ext.workspace.orNull?.asFile
                ?: error("rs.whisker: `whisker { workspace = file(...) }` is required.")
            val userPackage = ext.userPackage.orNull
                ?: error("rs.whisker: `whisker { userPackage = \"...\" }` is required.")

            val report = loadOrRefreshModulesReport(workspace, userPackage)

            report.modules.forEach { m ->
                m.android?.let { a ->
                    settings.include(":${m.crateName}")
                    settings.project(":${m.crateName}").projectDir = File(a.subprojectDir)
                }
            }

            // Hand off workspace + userPackage to the Project plugin
            // through a file under `<rootDir>/.whisker/`. Avoids a
            // BuildService (would require sharing a typed class
            // across classloaders, which the JAR split prevents).
            writeConfigForProjectPlugin(settings.rootDir, workspace, userPackage)
        }
    }

    private fun loadOrRefreshModulesReport(workspace: File, userPackage: String): ModulesReport {
        val cachePath = File(workspace, "target/whisker/module-info.json")
        val expectedHash = sha256OfCargoLock(workspace)

        if (cachePath.isFile && expectedHash != null) {
            try {
                val cached = ModulesReport.parse(cachePath)
                if (cached.cargoLockSha256 == expectedHash) {
                    return cached
                }
            } catch (_: Exception) {
                // Stale / corrupt cache — fall through to re-run.
            }
        }

        val text = runWhiskerBuildModules(workspace, userPackage)
        try {
            cachePath.parentFile?.mkdirs()
            cachePath.writeText(text)
        } catch (_: IOException) {
            // Caching is a perf opt — failing to write isn't fatal.
        }
        return ModulesReport.parse(text)
    }

    private fun sha256OfCargoLock(workspace: File): String? {
        val lock = File(workspace, "Cargo.lock")
        if (!lock.isFile) return null
        val md = MessageDigest.getInstance("SHA-256")
        val hash = md.digest(lock.readBytes())
        return hash.joinToString("") { "%02x".format(it) }
    }

    private fun runWhiskerBuildModules(workspace: File, userPackage: String): String {
        val proc = ProcessBuilder(
            "whisker-build",
            "modules",
            "--workspace=${workspace.absolutePath}",
            "--package=$userPackage",
        ).start()
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

    private fun writeConfigForProjectPlugin(
        rootDir: File,
        workspace: File,
        userPackage: String,
    ) {
        val dir = File(rootDir, ".whisker")
        dir.mkdirs()
        val cfg = File(dir, "config.properties")
        val props = Properties()
        props["workspace"] = workspace.absolutePath
        props["user_package"] = userPackage
        cfg.outputStream().use { props.store(it, "rs.whisker Settings plugin output") }
    }
}
