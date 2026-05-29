// `whisker-ksp` — the KSP `SymbolProcessor` that discovers
// `rs.whisker.runtime.Module` subclasses across the user app's
// compilation classpath and emits `<Module>Behaviors.kt` into
// the app's generated-source set.
//
// Pure Kotlin/JVM module. The processor itself isn't Android-aware;
// it just generates Kotlin source. The generated source IS Android-
// aware (imports Lynx + Android classes) but that lands inside the
// user app's gradle build, which has Android available.

plugins {
    kotlin("jvm")
    `java-library`
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    // KSP API the processor runs against. Major version must match
    // the Kotlin compiler version the user app is compiled with —
    // KSP 2.0.21-1.0.27 pairs with Kotlin 2.0.21.
    implementation("com.google.devtools.ksp:symbol-processing-api:2.0.21-1.0.27")
}
