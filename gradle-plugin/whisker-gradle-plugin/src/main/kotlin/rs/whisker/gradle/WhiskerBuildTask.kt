package rs.whisker.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import javax.inject.Inject

// One-shot wrapper around the `whisker-build android` CLI for a
// single (variant, abi) pair.
//
// The Rust binary handles the actual heavy lifting — cargo
// cross-compile, `.so` post-processing, module-system gradle
// subproject emission. This task exists so Gradle can:
//
//   * declare `jniLibs/<abi>/` as an `@OutputDirectory` (Gradle
//     knows what to clean + which AGP task this feeds into),
//   * fail the build with the same exit code path as any other
//     Gradle task when cargo fails.
//
// `whisker-build` is resolved off `PATH` — bootstrap with
// `cargo install whisker-build`. A future improvement is to download
// a pinned pre-built binary in `WhiskerPlugin.apply` so contributors
// don't need a Rust toolchain just to open the Android project.
abstract class WhiskerBuildTask : DefaultTask() {

    // `@Internal` rather than `@InputDirectory` so Gradle doesn't
    // walk the entire cargo workspace tree as a task input. The
    // workspace can contain `packages/*/build/` dirs that are
    // outputs of OTHER (Whisker module subproject) tasks, and
    // declaring the whole workspace as an input makes Gradle
    // refuse to run with "implicit dependency on
    // :whisker-router:generateReleaseResValues" etc.
    //
    // Cargo has its own incremental build detection (target/cache,
    // .fingerprint files), so we don't gain much by also having
    // Gradle UP-TO-DATE-check us. The task re-runs every time;
    // cargo skips the actual compile when nothing changed.
    @get:Internal
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
        // Fail fast with a clear message if `whisker-build` isn't on
        // PATH. Without this, Gradle reports the raw POSIX error
        // (`Cannot run program 'whisker-build': error=2, No such file
        // or directory`) which leaves the user guessing what to
        // install. Mirrors the `command -v` check the iOS Run Script
        // does up-front for the same reason.
        if (!isOnPath("whisker-build")) {
            error(
                "rs.whisker.gradle: 'whisker-build' is not on PATH. " +
                    "Install with: cargo install whisker-build " +
                    "(re-open Android Studio after install so it picks up the new PATH).",
            )
        }
        val ws = workspace.get().asFile.absolutePath
        // AGP's `addGeneratedSourceDirectory(task, ::jniLibsDir)`
        // hands the entire `jniLibsDir` to mergeJniLibFolders as a
        // jniLibs source root — and that contract demands an
        // `<abi>/<lib>.so` layout inside (the merge task throws
        // "not an ABI" if it sees raw .so files at the root). So
        // whisker-build places files into a nested `<abi>/` subdir
        // even though our task is already (variant, abi)-scoped.
        val abiSubdir = jniLibsDir.get().asFile.resolve(abi.get())
        abiSubdir.mkdirs()
        execOperations.exec {
            commandLine(
                "whisker-build",
                "android",
                "--workspace=$ws",
                "--package=${packageName.get()}",
                "--profile=${profile.get()}",
                "--abi=${abi.get()}",
                "--jni-libs-dir=${abiSubdir.absolutePath}",
                "--min-sdk=${minSdk.get()}",
            )
        }
    }

    private fun isOnPath(tool: String): Boolean {
        val pathEnv = System.getenv("PATH") ?: return false
        val sep = System.getProperty("path.separator") ?: ":"
        return pathEnv.split(sep).any { dir ->
            dir.isNotEmpty() && java.io.File(dir, tool).let { it.isFile && it.canExecute() }
        }
    }
}
