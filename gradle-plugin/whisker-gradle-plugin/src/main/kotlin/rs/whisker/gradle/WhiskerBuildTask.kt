package rs.whisker.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputDirectory
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import javax.inject.Inject

/// One-shot wrapper around the `whisker-build android` CLI for a
/// single (variant, abi) pair.
///
/// The Rust binary handles the actual heavy lifting — cargo
/// cross-compile, `.so` post-processing, module-system gradle
/// subproject emission. This task exists so Gradle can:
///
///   * declare the workspace `Cargo.toml` tree as an `@InputDirectory`
///     (UP-TO-DATE checks track Rust source edits),
///   * declare `jniLibs/<abi>/` as an `@OutputDirectory` (Gradle
///     knows what to clean + which AGP task this feeds into),
///   * fail the build with the same exit code path as any other
///     Gradle task when cargo fails.
///
/// `whisker-build` is resolved off `PATH` — bootstrap with
/// `cargo install whisker-build`. A future improvement is to download
/// a pinned pre-built binary in `WhiskerPlugin.apply` so contributors
/// don't need a Rust toolchain just to open the Android project.
abstract class WhiskerBuildTask : DefaultTask() {

    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val workspace: DirectoryProperty

    @get:Input
    abstract val packageName: Property<String>

    /// `debug` or `release` — maps to cargo `--profile`. AGP variants
    /// carry their own debuggable/release marker which `WhiskerPlugin`
    /// flattens into one of those two strings here.
    @get:Input
    abstract val profile: Property<String>

    /// One of `arm64-v8a` / `armeabi-v7a` / `x86_64` / `x86`. The
    /// Rust side maps this to a target triple via
    /// `whisker_build::android::abi_to_triple`.
    @get:Input
    abstract val abi: Property<String>

    /// Where the staged `.so` lands —
    /// `<variant>/jniLibs/<abi>/lib<package>.so`. AGP's
    /// `mergeJniLibFolders` task picks them up from here.
    @get:OutputDirectory
    abstract val jniLibsDir: DirectoryProperty

    @get:Input
    abstract val minSdk: Property<Int>

    @get:Inject
    abstract val execOperations: ExecOperations

    @TaskAction
    fun run() {
        val ws = workspace.get().asFile.absolutePath
        val jni = jniLibsDir.get().asFile.absolutePath
        execOperations.exec {
            commandLine(
                "whisker-build",
                "android",
                "--workspace=$ws",
                "--package=${packageName.get()}",
                "--profile=${profile.get()}",
                "--abi=${abi.get()}",
                "--jni-libs-dir=$jni",
                "--min-sdk=${minSdk.get()}",
            )
        }
    }
}
