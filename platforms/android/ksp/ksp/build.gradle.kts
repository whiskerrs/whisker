// `whisker-ksp` — the KSP `SymbolProcessor` that discovers
// `rs.whisker.runtime.Module` subclasses across the user app's
// compilation classpath and emits `<Module>Behaviors.kt` into
// the app's generated-source set.
//
// Pure Kotlin/JVM module. The processor itself isn't Android-aware;
// it just generates Kotlin source. The generated source IS Android-
// aware (imports Lynx + Android classes) but that lands inside the
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
    // KSP API the processor runs against. Major version must match
    // the Kotlin compiler version the user app is compiled with —
    // KSP 2.0.21-1.0.27 pairs with Kotlin 2.0.21.
    implementation("com.google.devtools.ksp:symbol-processing-api:2.0.21-1.0.27")
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
                        "for rs.whisker.runtime.Module subclasses and emits " +
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
