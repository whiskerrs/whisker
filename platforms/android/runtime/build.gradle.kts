plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    `maven-publish`
}

group = "rs.whisker"
version = "0.0.0-dev"

android {
    namespace = "rs.whisker.runtime.host"
    compileSdk = 34

    defaultConfig {
        minSdk = 24
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
            withJavadocJar()
        }
    }
}

dependencies {
    val moduleProject = if (rootProject.findProject(":module") != null) ":module" else ":whisker-module"
    api(project(moduleProject))
}

publishing {
    publications {
        register<MavenPublication>("release") {
            afterEvaluate { from(components["release"]) }
            artifactId = "whisker-runtime-android"
            pom {
                name.set("Whisker Android Host runtime")
                description.set("Native Android WhiskerView, scene projection, measurement, and paint implementation.")
                url.set("https://github.com/whiskerrs/whisker")
                licenses {
                    license {
                        name.set("MIT")
                        url.set("https://github.com/whiskerrs/whisker/blob/main/LICENSE")
                    }
                }
            }
        }
    }
    repositories {
        maven {
            name = "ghPages"
            url = uri(
                providers.gradleProperty("publishUrl")
                    .orElse("file://${rootProject.layout.buildDirectory.get()}/repo")
                    .get(),
            )
        }
    }
}
