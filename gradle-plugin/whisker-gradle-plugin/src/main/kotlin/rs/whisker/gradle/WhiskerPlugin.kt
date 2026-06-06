package rs.whisker.gradle

import com.android.build.api.dsl.CommonExtension
import com.android.build.api.variant.AndroidComponentsExtension
import org.gradle.api.Plugin
import org.gradle.api.Project

// Apply with `id("rs.whisker.gradle")` AFTER `com.android.application`
// (or `com.android.library`) in `app/build.gradle.kts`.
//
// What it does:
//
//   1. Creates the `whisker { ... }` extension on the project
//      ([`WhiskerExtension`]).
//   2. Hooks `AndroidComponentsExtension.onVariants` so every
//      Gradle variant (`debug`, `release`, custom flavors) gets a
//      [`WhiskerBuildTask`] per requested ABI. The task runs
//      `whisker-build android` to cross-compile + stage.
//   3. Adds the per-variant `jniLibs/<abi>/` dir to that variant's
//      sourceset, so `mergeJniLibFolders` picks the Rust dylib up
//      without the user wiring it in by hand.
//
// Heavy lifting (cargo cross-compile, NDK toolchain resolve,
// module-system gradle subproject emission, `.so` post-processing)
// lives in the `whisker-build` Rust binary. This plugin is just the
// AGP-integration shim — keeping the orchestration logic in Rust
// means `whisker run` / `whisker build` (CLI path) and Gradle Sync
// (IDE path) share the same code.
class WhiskerPlugin : Plugin<Project> {
    override fun apply(project: Project) {
        val ext = project.extensions.create("whisker", WhiskerExtension::class.java).apply {
            // Sensible default — modern Android, single arch. Multi-
            // arch fan-out is a future toggle once the consumer
            // demand shows up.
            abis.convention(listOf("arm64-v8a"))
        }

        val androidComponents = project.extensions.findByType(AndroidComponentsExtension::class.java)
            ?: error(
                "rs.whisker.gradle must be applied AFTER com.android.application " +
                    "(or com.android.library) — the AndroidComponentsExtension was not found."
            )

        androidComponents.onVariants { variant ->
            val agpDsl = project.extensions.getByType(CommonExtension::class.java)
            // Default minSdk to whatever AGP's DSL carries; the
            // `whisker { minSdk.set(...) }` override wins when set.
            val minSdkProvider = ext.minSdk.orElse(
                project.provider { agpDsl.defaultConfig.minSdk ?: 24 }
            )
            // AGP's `onVariants` gives a generic `Variant` — neither
            // `debuggable` nor `isMinifyEnabled` is on the common
            // interface, so distinguish on name. `release` /
            // `Release` is the canonical AGP build-type for
            // non-debug; anything else maps to cargo `debug`. The
            // user can still override `whisker { ... }` per flavor
            // in a future hook.
            val cargoProfile = if (variant.name.endsWith("release", ignoreCase = true))
                "release"
            else
                "debug"

            ext.abis.get().forEach { abi ->
                val taskName = "whiskerBuild${variant.name.replaceFirstChar { it.uppercase() }}${abi.toCamelCase()}"
                val jniLibsDir = project.layout.buildDirectory.dir(
                    "intermediates/whisker_jni_libs/${variant.name}/$abi"
                )
                val task = project.tasks.register(taskName, WhiskerBuildTask::class.java) {
                    group = "whisker"
                    description = "Cross-compile the Whisker Rust crate for $abi (${variant.name})."
                    workspace.set(ext.workspace)
                    packageName.set(ext.package_)
                    profile.set(project.provider { cargoProfile })
                    this.abi.set(abi)
                    this.jniLibsDir.set(jniLibsDir)
                    minSdk.set(minSdkProvider)
                }
                // Register the task's output dir as an extra
                // jniLibs sourceset for this variant. AGP wires it
                // into `merge<Variant>JniLibFolders` automatically.
                variant.sources.jniLibs?.addGeneratedSourceDirectory(
                    task,
                    WhiskerBuildTask::jniLibsDir,
                )
            }
        }
    }
}

private fun String.toCamelCase(): String =
    split('-', '_').joinToString("") { it.replaceFirstChar { c -> c.uppercase() } }
