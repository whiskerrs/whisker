package rs.whisker.gradle

import org.gradle.api.provider.Property
import org.gradle.api.services.BuildService
import org.gradle.api.services.BuildServiceParameters

// Carries the resolved module list + workspace config across
// Gradle's settings → project boundary.
//
// The Settings plugin populates `Params.report` + `Params.workspace`
// + `Params.userPackage` while it's still in Initialization phase
// (the BuildService is registered against `gradle.sharedServices`).
// The Project plugin grabs the same service in Configuration phase
// and reads the values to wire `implementation(project(":..."))` +
// the aggregator generation task.
//
// Why a BuildService rather than `gradle.extraProperties`: build
// services have first-class lifecycle handling (one instance per
// build, automatic shutdown), play nicely with Gradle's
// configuration cache, and don't pollute root-project state.
abstract class WhiskerModuleRegistry : BuildService<WhiskerModuleRegistry.Params> {
    interface Params : BuildServiceParameters {
        // Pretty-printed JSON the Settings plugin captured from
        // `whisker-build modules`. Kept as the raw string so the
        // BuildService stays trivially Serializable for the
        // configuration cache; the Project plugin re-parses on demand.
        val reportJson: Property<String>

        // Echoed back from the Settings extension so the Project
        // plugin can register `WhiskerBuildTask`s with the same
        // workspace + user-package values the user typed once.
        val workspace: Property<String>
        val userPackage: Property<String>
    }

    fun report(): ModulesReport = ModulesReport.parse(parameters.reportJson.get())
}
