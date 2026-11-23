    val localGradlePlugin = file("{{whisker_host_sdk_path}}/platforms/android/gradle-plugin")
    if (localGradlePlugin.isDirectory) {
        // A `File` argument binds to `Settings.includeBuild`, which cannot supply plugins.
        includeBuild(localGradlePlugin.path)
    }
