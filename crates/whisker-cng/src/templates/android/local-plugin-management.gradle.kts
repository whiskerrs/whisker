    val localGradlePlugin = file("{{whisker_host_sdk_path}}/platforms/android/gradle-plugin")
    if (localGradlePlugin.isDirectory) {
        includeBuild(localGradlePlugin)
    }
