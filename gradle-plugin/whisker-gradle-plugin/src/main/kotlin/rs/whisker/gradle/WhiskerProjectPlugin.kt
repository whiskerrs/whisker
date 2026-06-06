package rs.whisker.gradle

import com.android.build.api.dsl.CommonExtension
import com.android.build.api.variant.AndroidComponentsExtension
import org.gradle.api.Plugin
import org.gradle.api.Project
import java.io.File

// Project-scope plugin (id="rs.whisker.gradle"). Normally users don't
// apply this directly — the Settings plugin (id="rs.whisker",
// [WhiskerPlugin]) auto-applies it on every project that has AGP
// applied via `gradle.beforeProject`. The standalone ID stays alive
// so people who want explicit, opt-in behaviour (or who can't use a
// Settings plugin, e.g. multi-build composites with quirky
// classloader rules) still have a way in.
//
// On apply:
//   1. Look up the `WhiskerModuleRegistry` BuildService the Settings
//      plugin populated.
//   2. Add `implementation(project(":<crate>"))` for every Whisker
//      module that has `android: { ... }` in the report.
//   3. Per variant: a `WhiskerModuleBehaviorsTask` (aggregator
//      Kotlin) + a `WhiskerBuildTask` per ABI (cargo cross-compile).
//      Both are wired into AGP via `addGeneratedSourceDirectory`.
class WhiskerProjectPlugin : Plugin<Project> {
    override fun apply(project: Project) {
        val androidComponents =
            project.extensions.findByType(AndroidComponentsExtension::class.java)
                ?: error(
                    "rs.whisker.gradle must be applied AFTER com.android.application " +
                        "(or com.android.library) — the AndroidComponentsExtension was not found.",
                )

        // Re-register-if-absent returns the same instance the Settings
        // plugin registered; the parameters lambda is ignored when the
        // service already exists. Typed Provider<WhiskerModuleRegistry>
        // without an unchecked cast.
        val registryProvider = project.gradle.sharedServices.registerIfAbsent(
            WHISKER_MODULE_REGISTRY_NAME,
            WhiskerModuleRegistry::class.java,
        ) {
            // Defensive fallback — if this is reached it means the
            // Settings plugin never ran. Surface a clearer error than
            // a NoSuchElementException on `parameters.reportJson.get()`.
            error(
                "rs.whisker.gradle requires the rs.whisker Settings plugin to have " +
                    "run first. Add `plugins { id(\"rs.whisker\") version ... }` to " +
                    "settings.gradle.kts.",
            )
        }
        val registry = registryProvider.get()
        val report = registry.report()
        val workspaceFile = File(registry.parameters.workspace.get())
        val userPackageStr = registry.parameters.userPackage.get()

        // Module deps onto this project (each android-capable Whisker
        // module is a Gradle subproject the Settings plugin
        // `include()`'d).
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
            variant.sources.kotlin?.addGeneratedSourceDirectory(
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
                }
                variant.sources.jniLibs?.addGeneratedSourceDirectory(
                    task,
                    WhiskerBuildTask::jniLibsDir,
                )
            }
        }
    }
}

private fun String.titlecaseFirst(): String = replaceFirstChar { it.uppercase() }

private fun String.toCamelCase(): String =
    split('-', '_').joinToString("") { it.titlecaseFirst() }
