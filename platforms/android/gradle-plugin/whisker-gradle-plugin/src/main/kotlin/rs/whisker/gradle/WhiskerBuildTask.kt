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

// One-shot wrapper around the `whisker build-android` CLI for a
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
// `whisker` is resolved off `PATH` — bootstrap with
// `cargo install whisker-cli`.
abstract class WhiskerBuildTask : DefaultTask() {

    // `@Internal` rather than `@InputDirectory` so Gradle doesn't
    // walk the entire cargo workspace tree as a task input. The
    // workspace can contain `packages/*/build/` dirs that are
    // outputs of OTHER (Whisker module subproject) tasks, and
    // declaring the whole workspace as an input makes Gradle
    // refuse to run with "implicit dependency on
    // :whisker-router:generateReleaseResValues" etc.
    //
    // Cargo has its own incremental build detection, so the task
    // re-runs every time and cargo skips the compile when nothing
    // changed.
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
        val whiskerCli = System.getenv("WHISKER_CLI")
            ?.takeIf { it.isNotBlank() }
            ?: "whisker"
        // Fail fast with a clear message when the selected CLI cannot
        // execute. `whisker run/build` supplies its own absolute path;
        // Android Studio builds fall back to resolving `whisker` on PATH.
        // Without this check Gradle only reports a raw POSIX spawn error.
        if (!isExecutable(whiskerCli)) {
            error(
                "rs.whisker.gradle: Whisker CLI '$whiskerCli' is not executable. " +
                    "Install with: cargo install whisker-cli " +
                    "(re-open Android Studio after install so it picks up the new PATH).",
            )
        }
        val ws = workspace.get().asFile.absolutePath
        // AGP's `addGeneratedSourceDirectory(task, ::jniLibsDir)`
        // hands the entire `jniLibsDir` to mergeJniLibFolders as a
        // jniLibs source root — and that contract demands an
        // `<abi>/<lib>.so` layout inside (the merge task throws
        // "not an ABI" if it sees raw .so files at the root). So
        // `whisker build-android` places files into a nested `<abi>/` subdir
        // even though our task is already (variant, abi)-scoped.
        val abiSubdir = jniLibsDir.get().asFile.resolve(abi.get())
        abiSubdir.mkdirs()
        // `whisker run` sets WHISKER_FEATURES=whisker/hot-reload (space-
        // separated if multiple) on the gradle subprocess so the user
        // dylib carries the dev-runtime WebSocket client. Without it the
        // app never reports its `aslr_reference` to the dev-server, and
        // every change falls through to a Tier 2 cold rebuild + relaunch.
        // `./gradlew assembleRelease` runs from CI with the env unset;
        // production builds skip the dev-runtime entirely.
        val featureArgs = (System.getenv("WHISKER_FEATURES") ?: "")
            .split(Regex("\\s+"))
            .filter { it.isNotEmpty() }
            .flatMap { listOf("--features", it) }
        execOperations.exec {
            commandLine(
                listOf(
                    whiskerCli,
                    "build-android",
                    "--workspace=$ws",
                    "--package=${packageName.get()}",
                    "--profile=${profile.get()}",
                    "--abi=${abi.get()}",
                    "--jni-libs-dir=${abiSubdir.absolutePath}",
                    "--min-sdk=${minSdk.get()}",
                ) + featureArgs,
            )
            // `exec {}` inherits the gradle daemon's env, but that env
            // is captured at fork-time and may predate `whisker run`
            // setting `WHISKER_TUI=1` (especially with `--daemon` reuse
            // across sessions). Without the explicit forward the child
            // runs `whisker_build::ui::step` in non-TUI mode and emits
            // a duplicate progress row for every step.
            for (name in listOf("WHISKER_TUI", "WHISKER_VERBOSE")) {
                System.getenv(name)?.let { value -> environment(name, value) }
            }
        }
    }

    private fun isExecutable(tool: String): Boolean {
        val direct = java.io.File(tool)
        if (direct.isAbsolute || tool.contains(java.io.File.separatorChar)) {
            return direct.isFile && direct.canExecute()
        }
        val pathEnv = System.getenv("PATH") ?: return false
        val sep = System.getProperty("path.separator") ?: ":"
        return pathEnv.split(sep).any { dir ->
            dir.isNotEmpty() && java.io.File(dir, tool).let { it.isFile && it.canExecute() }
        }
    }
}
