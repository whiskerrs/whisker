package rs.whisker.gradle

import com.android.build.api.dsl.CommonExtension
import com.android.build.api.variant.AndroidComponentsExtension
import org.gradle.api.Plugin
import org.gradle.api.Project
import java.io.File
import java.util.Properties

// Project-scope plugin (id `rs.whisker.gradle`). Users declare in
// `app/build.gradle.kts` AFTER `com.android.application` /
// `com.android.library`:
//
// ```kotlin
// plugins {
//     id("com.android.application")
//     id("org.jetbrains.kotlin.android")
//     id("rs.whisker.gradle")           // version inherited from settings
// }
// android { ... }
// ```
//
// `version` is inherited automatically from the version pin the user
// already declared on `rs.whisker` in `settings.gradle.kts` — Gradle
// caches the pluginManagement resolution across the build.
//
// On apply this plugin:
//   1. Reads `<rootDir>/.whisker/config.properties` (workspace +
//      userPackage) the Settings plugin wrote.
//   2. Reads `<workspace>/target/whisker/module-info.json` (the
//      module list).
//   3. For each module with an Android subproject:
//      `implementation(project(":<crate>"))`.
//   4. Per variant: register `WhiskerModuleBehaviorsTask` (aggregator
//      Kotlin) + `WhiskerBuildTask` per ABI (cargo cross-compile).
//      Both are wired into the variant via
//      `addGeneratedSourceDirectory`.
//
// If the user applies `rs.whisker.gradle` without the Settings plugin
// having run first, the config file lookup fails with a clear error
// message pointing at the missing `id("rs.whisker")` declaration.
class WhiskerProjectPlugin : Plugin<Project> {
    override fun apply(project: Project) {
        val androidComponents =
            project.extensions.findByType(AndroidComponentsExtension::class.java)
                ?: error(
                    "rs.whisker.gradle must be applied AFTER com.android.application " +
                        "(or com.android.library) — the AndroidComponentsExtension was not found.",
                )

        val (workspaceFile, userPackageStr) = readConfig(project.rootDir)
        val report = readModulesReport(workspaceFile)

        // Module deps onto this project (each android-capable Whisker
        // module is a Gradle subproject the Settings plugin `include`d).
        report.modules
            .filter { it.android != null }
            .forEach { m ->
                project.dependencies.add(
                    "implementation",
                    project.dependencies.project(mapOf("path" to ":${m.crateName}")),
                )
            }

        androidComponents.onVariants { variant ->
            val agpDsl = project.extensions.getByType(CommonExtension::class.java)
            val minSdkProvider = project.provider { agpDsl.defaultConfig.minSdk ?: 24 }

            val cargoProfile = if (variant.name.endsWith("release", ignoreCase = true)) {
                "release"
            } else {
                "debug"
            }

            // Aggregator task — generates
            // `rs/whisker/runtime/generated/WhiskerModuleBehaviors.kt`
            // into a per-variant build dir. AGP wires the dir into
            // `compile<Variant>Kotlin` via addGeneratedSourceDirectory.
            val aggregatorOut = project.layout.buildDirectory.dir(
                "generated/whisker/${variant.name}/kotlin",
            )
            val aggregatorTask = project.tasks.register(
                "whiskerGenerateAggregator${variant.name.titlecaseFirst()}",
                WhiskerModuleBehaviorsTask::class.java,
            ) {
                group = "whisker"
                description = "Generate WhiskerModuleBehaviors.kt for ${variant.name}."
                behaviorsClasses.set(report.modules.mapNotNull { it.android?.behaviorsClass })
                outputDir.set(aggregatorOut)
            }
            // AGP 8.6 routes the Kotlin Android plugin's generated
            // sources through `variant.sources.java`, not
            // `variant.sources.kotlin`. The kotlin compile pulls from
            // both, but only `java`'s `addGeneratedSourceDirectory`
            // call propagates the implicit task dependency on the
            // generator into `compile<Variant>Kotlin`. Wiring through
            // `.kotlin` alone leaves the task unscheduled and the
            // compile fails with "Unresolved reference
            // 'WhiskerModuleBehaviors'" even though the file is on
            // disk after a manual `./gradlew :app:whiskerGenerateAggregatorRelease`.
            variant.sources.java?.addGeneratedSourceDirectory(
                aggregatorTask,
                WhiskerModuleBehaviorsTask::outputDir,
            )

            // Cargo + .so staging per (variant, ABI).
            val abis = listOf("arm64-v8a")
            abis.forEach { abi ->
                val taskName =
                    "whiskerBuild${variant.name.titlecaseFirst()}${abi.toCamelCase()}"
                val jniLibsDir = project.layout.buildDirectory.dir(
                    "intermediates/whisker_jni_libs/${variant.name}/$abi",
                )
                val task = project.tasks.register(taskName, WhiskerBuildTask::class.java) {
                    group = "whisker"
                    description =
                        "Cross-compile the Whisker Rust crate for $abi (${variant.name})."
                    workspace.fileValue(workspaceFile)
                    packageName.set(userPackageStr)
                    profile.set(cargoProfile)
                    this.abi.set(abi)
                    this.jniLibsDir.set(jniLibsDir)
                    minSdk.set(minSdkProvider)
                    // The Rust sources are `@Internal` (not declared inputs),
                    // so Gradle can't tell when they change and would mark this
                    // task UP-TO-DATE and skip the cargo recompile — shipping a
                    // stale `.so` (#260). Always run; cargo is the single
                    // authority and skips the actual rustc when nothing changed.
                    outputs.upToDateWhen { false }
                }
                variant.sources.jniLibs?.addGeneratedSourceDirectory(
                    task,
                    WhiskerBuildTask::jniLibsDir,
                )
            }
        }
    }

    private fun readConfig(rootDir: File): Pair<File, String> {
        val cfg = File(rootDir, ".whisker/config.properties")
        if (!cfg.isFile) {
            error(
                "rs.whisker.gradle: missing ${cfg.absolutePath}. " +
                    "Did you forget `plugins { id(\"rs.whisker\") version ... }` in settings.gradle.kts?",
            )
        }
        val props = Properties().apply { cfg.inputStream().use { load(it) } }
        val ws = props.getProperty("workspace")
            ?: error("rs.whisker.gradle: ${cfg.absolutePath} missing `workspace` key")
        val pkg = props.getProperty("user_package")
            ?: error("rs.whisker.gradle: ${cfg.absolutePath} missing `user_package` key")
        return File(ws) to pkg
    }

    private fun readModulesReport(workspace: File): ModulesReport {
        val path = File(workspace, "target/whisker/module-info.json")
        if (!path.isFile) {
            error(
                "rs.whisker.gradle: missing ${path.absolutePath} — the Settings " +
                    "plugin should have written this. Re-run Sync.",
            )
        }
        return ModulesReport.parse(path)
    }
}

private fun String.titlecaseFirst(): String = replaceFirstChar { it.uppercase() }

private fun String.toCamelCase(): String =
    split('-', '_').joinToString("") { it.titlecaseFirst() }
