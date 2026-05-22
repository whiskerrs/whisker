// SwiftPM build-tool plugin that invokes the `WhiskerElementsCodegen`
// executable (Sources/WhiskerElementsCodegen) against every `.swift`
// file in the consuming target's source set.
//
// Activates when a target's `Package.swift` declares
// `plugins: [.plugin(name: "WhiskerElementsCodegenPlugin",
//                    package: "whisker-ios-macros")]`.
// The generated `WhiskerModuleBehaviors.swift` lands in the plugin's
// per-target work directory; SwiftPM automatically adds it to the
// target's compilation.
//
// Companion to the Android KSP processor — same shape, same output
// (a `WhiskerModuleBehaviors` symbol with a single `registerAll`).

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
        let output = context.pluginWorkDirectory
            .appending("WhiskerModuleBehaviors.swift")

        let inputs = sourceTarget.sourceFiles
            .filter { $0.path.extension == "swift" }
            .map { $0.path }

        var arguments: [String] = ["--output", output.string]
        arguments.append(contentsOf: inputs.map { $0.string })

        return [
            .buildCommand(
                displayName: "Generate WhiskerModuleBehaviors.swift",
                executable: tool.path,
                arguments: arguments,
                inputFiles: inputs,
                outputFiles: [output]
            )
        ]
    }
}
