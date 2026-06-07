package rs.whisker.gradle

import groovy.json.JsonSlurper
import java.io.File
import java.io.Serializable

// Mirror of the JSON schema `whisker-build modules` writes to stdout.
// Kept as plain `data class`es with explicit `fromMap` constructors so
// the BuildService can carry them through Gradle's configuration cache
// (Serializable).
//
// Why JsonSlurper + manual mapping rather than kotlinx.serialization or
// moshi: JsonSlurper is already on Gradle's classpath (Gradle ships
// `groovy-json`), so the plugin's published JAR drags no extra deps.
// The schema is small enough that the boilerplate cost is real but
// modest.

data class ModulesReport(
    val cargoLockSha256: String,
    val userPackage: String,
    val modules: List<ModuleEntry>,
) : Serializable {
    companion object {
        private const val serialVersionUID: Long = 1L

        fun parse(file: File): ModulesReport = fromMap(JsonSlurper().parse(file) as Map<*, *>)

        fun parse(text: String): ModulesReport =
            fromMap(JsonSlurper().parseText(text) as Map<*, *>)

        @Suppress("UNCHECKED_CAST")
        fun fromMap(map: Map<*, *>): ModulesReport = ModulesReport(
            cargoLockSha256 = map["cargo_lock_sha256"] as String,
            userPackage = map["user_package"] as String,
            modules = (map["modules"] as List<Map<*, *>>).map(ModuleEntry::fromMap),
        )
    }
}

data class ModuleEntry(
    val crateName: String,
    val manifestDir: String,
    val android: AndroidEntry?,
    val ios: IosEntry?,
) : Serializable {
    companion object {
        private const val serialVersionUID: Long = 1L

        fun fromMap(map: Map<*, *>): ModuleEntry = ModuleEntry(
            crateName = map["crate_name"] as String,
            manifestDir = map["manifest_dir"] as String,
            android = (map["android"] as Map<*, *>?)?.let(AndroidEntry::fromMap),
            ios = (map["ios"] as Map<*, *>?)?.let(IosEntry::fromMap),
        )
    }
}

data class AndroidEntry(
    val subprojectDir: String,
    val behaviorsClass: String,
) : Serializable {
    companion object {
        private const val serialVersionUID: Long = 1L

        fun fromMap(map: Map<*, *>): AndroidEntry = AndroidEntry(
            subprojectDir = map["subproject_dir"] as String,
            behaviorsClass = map["behaviors_class"] as String,
        )
    }
}

data class IosEntry(
    val swiftModule: String?,
    val nativeSources: List<String>,
    val swiftSources: List<String>,
) : Serializable {
    companion object {
        private const val serialVersionUID: Long = 1L

        @Suppress("UNCHECKED_CAST")
        fun fromMap(map: Map<*, *>): IosEntry = IosEntry(
            swiftModule = map["swift_module"] as String?,
            nativeSources = (map["native_sources"] as List<String>?).orEmpty(),
            swiftSources = (map["swift_sources"] as List<String>?).orEmpty(),
        )
    }
}
