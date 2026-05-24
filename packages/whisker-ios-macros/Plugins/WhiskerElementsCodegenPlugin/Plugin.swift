// SwiftPM build-tool plugin that invokes the `WhiskerElementsCodegen`
// executable (Sources/WhiskerElementsCodegen) against every `.swift`
// file in the consuming target's source set.
//
// Activates when a target's `Package.swift` declares
// `plugins: [.plugin(name: "WhiskerElementsCodegenPlugin",
//                    package: "whisker-ios-macros")]`.
//
// Phase 7-Φ.G: applied per-module — each module package adds the
// plugin to its own SwiftPM target. The codegen emits a top-level
// `_whiskerRegisterModules_<TargetName>()` function (uniquely
// named per module to avoid linker conflicts across modules) plus
// the per-target `<TargetName>+Generated.swift` file. The
// whisker-build-generated aggregator imports each module and
// calls every per-module register fn from its top-level
// `WhiskerModuleBehaviors.registerAll()`.
//
// Companion to the Android KSP processor — same shape, same
// per-subproject-per-app-aggregator split.

import Foundation
import PackagePlugin

@main
struct WhiskerElementsCodegenPlugin: BuildToolPlugin {
    func createBuildCommands(
        context: PluginContext,
        target: Target
    ) throws -> [Command] {
        // Only run for source-module targets — skip binary targets,
        // plugins, etc. that wouldn't carry `.swift` files.
        guard let sourceTarget = target as? SourceModuleTarget else {
            return []
        }

        let tool = try context.tool(named: "WhiskerElementsCodegen")
        // Output file name is the target name + `+Generated.swift`.
        // Filename uniqueness across modules isn't strictly required
        // (each SwiftPM target has its own work dir), but using the
        // target name in the filename keeps the build log readable
        // when multiple modules log "Generate <X>+Generated.swift"
        // simultaneously.
        let outputFileName = "\(sourceTarget.name)+Generated.swift"
        let output = context.pluginWorkDirectory.appending(outputFileName)

        let inputs = sourceTarget.sourceFiles
            .filter { $0.path.extension == "swift" }
            .map { $0.path }

        // `context.package.displayName` returns the `name:` declared
        // in the module's Package.swift. By convention each module
        // package's Package.swift names itself after its cargo crate
        // (kebab-case, e.g. "whisker-hello-element"), so this is the
        // tag namespace string we prepend to element registration
        // calls — matching what the Rust-side
        // `#[whisker::native_element]` proc macro emits via
        // `env!("CARGO_PKG_NAME")`. Phase 7-Φ.H.2.
        var arguments: [String] = [
            "--target-name", sourceTarget.name,
            "--crate-name", context.package.displayName,
            "--output", output.string,
        ]
        arguments.append(contentsOf: inputs.map { $0.string })

        return [
            .buildCommand(
                displayName: "Generate \(outputFileName)",
                executable: tool.path,
                arguments: arguments,
                inputFiles: inputs,
                outputFiles: [output]
            )
        ]
    }
}
