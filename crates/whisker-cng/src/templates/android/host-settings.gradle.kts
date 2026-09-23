val whiskerWorkspace = file("{{whisker_workspace_path}}")
// Refresh before the Settings plugin reads its Cargo.lock cache. Feature and
// path-manifest edits need not change Cargo.lock. Keeping this in the generated
// project also supports the currently published Gradle plugin.
val whiskerCli = System.getenv("WHISKER_CLI")?.takeIf { it.isNotBlank() } ?: "whisker"
val moduleRefresh = ProcessBuilder(
    whiskerCli, "modules",
    "--workspace=${whiskerWorkspace.absolutePath}",
    "--package={{whisker_user_package}}", "--write-cache",
).inheritIO().start()
check(moduleRefresh.waitFor() == 0) { "Whisker module discovery failed" }

whisker {
    workspace = whiskerWorkspace
    userPackage = "{{whisker_user_package}}"
}
val localAndroidSdk = whiskerWorkspace.resolve("platforms/android")
if (localAndroidSdk.isDirectory) {
    include(":whisker-module")
    project(":whisker-module").projectDir = localAndroidSdk.resolve("module")
    include(":whisker-runtime")
    project(":whisker-runtime").projectDir = localAndroidSdk.resolve("runtime")
}
val localKsp = whiskerWorkspace.resolve("platforms/android/ksp")
if (localKsp.isDirectory) {
    includeBuild(localKsp) {
        dependencySubstitution {
            substitute(module("rs.whisker:ksp")).using(project(":ksp"))
        }
    }
}
