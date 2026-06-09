pluginManagement {
    repositories {
        gradlePluginPortal()
        google()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()

        // Lynx fork AARs — `rs.whisker:lynx-{android,base-android,
        // trace-android,service-api-android}:<ver>` + primjs.
        // Read at build time when `-PwhiskerSdkRelease=true` flips the
        // module / whisker-runtime AAR deps from the flatDir
        // `:LynxAndroid@aar` form to Maven coords. The repo is always
        // declared so a developer running `./gradlew publishToMavenLocal`
        // doesn't have to hand-add it.
        maven {
            name = "lynxForkGhPages"
            url = uri("https://whiskerrs.github.io/lynx/maven")
        }

        // flatDir kept for legacy CLI flows that staged Lynx aars
        // into the workspace. Modern `whisker run` lets gradle pull
        // them from the Maven repo above transitively via the SDK
        // pom, so this directory is typically empty — declared
        // anyway because Gradle accepts empty flatDirs and the SDK
        // publish CI relies on the Maven fallback once
        // `-PwhiskerSdkRelease=true` is set.
        flatDir {
            dirs(rootDir.parentFile.parentFile.resolve("target/lynx-android"))
        }
    }
}

rootProject.name = "whisker-runtime-android"
include(":module")
include(":whisker-runtime")
