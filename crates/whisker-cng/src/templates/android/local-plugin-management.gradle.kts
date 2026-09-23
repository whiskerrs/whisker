    val localGradlePlugin = file("{{whisker_workspace_path}}/platforms/android/gradle-plugin")
    if (localGradlePlugin.isDirectory) {
        includeBuild(localGradlePlugin)
    }
