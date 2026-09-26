// `whisker-ksp` — the KSP `SymbolProcessor` that discovers
// `rs.whisker.runtime.Module` subclasses across the user app's
// compilation classpath and emits `<Module>Behaviors.kt` into
// the app's generated-source set.
//
// Pure Kotlin/JVM module. The processor itself isn't Android-aware;
// it just generates Kotlin source. The generated source IS Android-
// aware (imports Android Host classes) but that lands inside the
// user app's gradle build, which has Android available.
//
// Published as `rs.whisker:ksp` (short name picked because the
// user-side declaration `ksp("rs.whisker:ksp:<ver>")` reads
// naturally).

plugins {
    kotlin("jvm")
    `java-library`
    `maven-publish`
}

group = "rs.whisker"
version = "0.0.0-dev"

kotlin {
    jvmToolchain(17)
}

dependencies {
    // KSP2 versions independently of Kotlin; keep it in step with the modules' KSP plugin.
    implementation("com.google.devtools.ksp:symbol-processing-api:2.3.11")
}

java {
    withSourcesJar()
    withJavadocJar()
}

publishing {
    publications {
        register<MavenPublication>("maven") {
            from(components["java"])
            artifactId = "ksp"
            pom {
                name.set("Whisker KSP processor")
                description.set(
                    "KSP SymbolProcessor that scans the user app's classpath " +
                        "for @rs.whisker.runtime.WhiskerModule declarations and emits " +
                        "<Module>Behaviors.kt aggregator source.",
                )
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
            url = uri(providers.gradleProperty("publishUrl").orElse("file://${rootProject.layout.buildDirectory.get()}/repo").get())
        }
    }
}
