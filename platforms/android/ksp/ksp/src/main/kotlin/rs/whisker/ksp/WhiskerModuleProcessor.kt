package rs.whisker.ksp

import com.google.devtools.ksp.processing.CodeGenerator
import com.google.devtools.ksp.processing.Dependencies
import com.google.devtools.ksp.processing.KSPLogger
import com.google.devtools.ksp.processing.Resolver
import com.google.devtools.ksp.processing.SymbolProcessor
import com.google.devtools.ksp.processing.SymbolProcessorEnvironment
import com.google.devtools.ksp.processing.SymbolProcessorProvider
import com.google.devtools.ksp.symbol.ClassKind
import com.google.devtools.ksp.symbol.KSAnnotated
import com.google.devtools.ksp.symbol.KSClassDeclaration
import com.google.devtools.ksp.symbol.Modifier

/**
 * KSP processor that scans each module subproject's compilation for
 * every concrete `@rs.whisker.runtime.WhiskerModule` declaration (the
 * ModuleDefinition DSL base class) and generates a per-subproject
 * `rs.whisker.runtime.generated.<ModuleName>Behaviors` Kotlin object
 * whose `registerAll()` installs the module in Whisker's registries.
 *
 * Discovery is annotation-based and type checked: `@WhiskerModule` is the
 * explicit registration signal, and every annotated class must extend
 * `rs.whisker.runtime.Module`.
 *
 * For each annotated module the generated code instantiates it, assigns its
 * fully-qualified name, and calls `.registerWithWhisker()`. The common
 * registrar handles both native Views and view-less function modules.
 *
 * The generated object's symbol matches what
 * `WhiskerApplication.onCreate()` already invokes — see
 * `crates/whisker-cng/src/templates/android/app/src/main/kotlin/
 * Application.kt`.
 */
public class WhiskerModuleProcessor(
    private val codeGenerator: CodeGenerator,
    private val logger: KSPLogger,
    /**
     * Per-subproject KSP run identifier. Passed via
     * Gradle's `ksp { arg("whisker.moduleName", "<Name>") }` in each
     * Whisker module's `build.gradle.kts`. The processor uses this
     * to name the generated file (`<ModuleName>Behaviors.kt`) and
     * the top-level Kotlin object inside it, so two modules linked
     * into the same user-app composite build don't shadow each
     * other's `registerAll()` entry point.
     *
     * `null` falls back to the `WhiskerModuleBehaviors` name, used by
     * user apps that run KSP themselves.
     */
    private val moduleName: String?,
    /**
     * Cargo crate name (e.g. "whisker-hello-element"), passed via
     * Gradle's `ksp { arg("whisker.crateName", "<crate>") }` in
     * each Whisker module's `build.gradle.kts`. Used as the
     * element tag namespace so two unrelated modules' identical
     * local names don't collide in the Whisker registry.
     * `null` defaults to no namespace prefix.
     */
    private val crateName: String?,
) : SymbolProcessor {

    /** FQN of the base class every annotated Whisker module must extend. */
    private val moduleBaseFqn = "rs.whisker.runtime.Module"
    private val moduleAnnotationFqn = "rs.whisker.runtime.WhiskerModule"

    /**
     * KSP invokes `process` at least twice per compilation: once
     * when the user code is first processed (sources visible) and
     * again after generated code has been integrated. The `generated`
     * guard avoids double-writing the file on the second invocation.
     */
    private var generated = false

    override fun process(resolver: Resolver): List<KSAnnotated> {
        if (generated) return emptyList()

        val dslModuleSymbols = resolver
            .getSymbolsWithAnnotation(moduleAnnotationFqn)
            .filterIsInstance<KSClassDeclaration>()
            .filter { declaration ->
                val valid =
                    declaration.classKind == ClassKind.CLASS &&
                        !declaration.modifiers.contains(Modifier.ABSTRACT) &&
                        declaration.qualifiedName?.asString() != moduleBaseFqn &&
                        extendsModuleBase(declaration)
                if (!valid) {
                    logger.error(
                        "@WhiskerModule must annotate a concrete subclass of $moduleBaseFqn",
                        declaration,
                    )
                }
                valid
            }
            .toList()

        // Always write the file, even when empty, so the user app's
        // `Application.onCreate()` call to
        // `<Module>Behaviors.registerAll()` always resolves — mirrors
        // the iOS-side `WhiskerModuleBehaviors.swift` policy.
        writeBehavioursFile(dslModuleSymbols)
        generated = true

        return emptyList()
    }

    /** Uses KSP's semantic resolver, including indirect inheritance. */
    private fun extendsModuleBase(cls: KSClassDeclaration): Boolean {
        for (superRef in cls.superTypes) {
            val superType = superRef.resolve()
            val superDecl = superType.declaration as? KSClassDeclaration ?: continue
            if (superDecl.qualifiedName?.asString() == moduleBaseFqn) return true
            if (extendsModuleBase(superDecl)) return true
        }
        return false
    }

    private fun writeBehavioursFile(dslModules: List<KSClassDeclaration>) {
        // `Dependencies(aggregating = true, *sourceFiles)` makes the
        // generated file invalidate when ANY of the input source
        // files changes (add/remove of an annotated module declaration).
        // Important for incremental compilation — without
        // `aggregating = true` KSP wouldn't re-run when a new
        // annotated module appears.
        val sourceFiles = dslModules.mapNotNull { it.containingFile }
        val dependencies = Dependencies(aggregating = true, *sourceFiles.toTypedArray())

        // Per-subproject KSP runs pass `whisker.moduleName` so each
        // module's compilation produces its own uniquely-named
        // `<ModuleName>Behaviors.kt` — the user app's
        // whisker-build-generated aggregator imports each and chains
        // the per-module `registerAll()` calls. Without it the shared
        // `WhiskerModuleBehaviors` name keeps user-app-level KSP working.
        val behaviorsObjectName = moduleName?.let { "${it}Behaviors" } ?: "WhiskerModuleBehaviors"

        codeGenerator.createNewFile(
            dependencies = dependencies,
            packageName = "rs.whisker.runtime.generated",
            fileName = behaviorsObjectName,
            extensionName = "kt",
        ).bufferedWriter().use { w ->
            w.appendLine("// AUTO-GENERATED by `whisker-ksp` (rs.whisker.ksp.WhiskerModuleProcessor).")
            w.appendLine("// DO NOT EDIT — applies/removes happen automatically on next compile.")
            w.appendLine("//")
            w.appendLine("// Sourced from `@WhiskerModule` declarations in this")
            w.appendLine("// Whisker module subproject. Modules register under the fully-qualified name")
            w.appendLine("// `${crateName ?: "<no-namespace>"}:<Name>` — the namespace is the")
            w.appendLine("// cargo crate name passed via `ksp { arg(\"whisker.crateName\", \"…\") }`")
            w.appendLine("// so two modules can both declare a `Hello` element without colliding.")
            w.appendLine("//")
            w.appendLine("// WhiskerModule registrations: ${dslModules.size}")
            w.appendLine()
            w.appendLine("package rs.whisker.runtime.generated")
            w.appendLine()
            w.appendLine("import rs.whisker.runtime.registerWithWhisker")
            w.appendLine("import java.util.concurrent.atomic.AtomicBoolean")
            w.appendLine()
            w.appendLine("public object $behaviorsObjectName {")
            w.appendLine("    private val registered = AtomicBoolean(false)")
            w.appendLine()
            w.appendLine("    @JvmStatic")
            w.appendLine("    public fun registerAll() {")
            w.appendLine("        if (!registered.compareAndSet(false, true)) return")
            if (dslModules.isEmpty()) {
                w.appendLine("        // (no @WhiskerModule declaration found)")
            }

            // `registerWithWhisker()` handles both native-View and view-less
            // module definitions, so code generation has one registration
            // path on every Host.
            for (cls in dslModules) {
                val fqn = cls.qualifiedName?.asString()
                if (fqn == null) {
                    logger.warn(
                        "@WhiskerModule declaration has no qualified name; skipping",
                        cls,
                    )
                    continue
                }
                val simple = cls.simpleName.asString()
                val instanceLocal = "_dsl_${simple}"
                val defLocal = "_dsl_def_${simple}"
                val viewLocal = "_dsl_view_${simple}"
                val nameLocal = "_dsl_name_${simple}"
                w.appendLine("        run {")
                w.appendLine("            val $instanceLocal = $fqn()")
                w.appendLine("            val $defLocal = $instanceLocal.definitionLazy")
                w.appendLine("            val $nameLocal = $defLocal.name")
                w.appendLine("            val $viewLocal = $defLocal.view")
                w.appendLine("            requireNotNull($nameLocal) { \"ModuleDefinition requires Name\" }")
                // Every module records its fully-qualified name so event and
                // function routing can find the same instance.
                val tagPrefix = if (crateName != null) "$crateName:" else ""
                w.appendLine("            val qualifiedName = if ('/' in $nameLocal) $nameLocal else \"$tagPrefix\" + $nameLocal")
                w.appendLine("            $instanceLocal.qualifiedName = qualifiedName")
                val crateArg = if (crateName != null) "\"$crateName\"" else "null"
                w.appendLine("            $instanceLocal.registerWithWhisker($crateArg)")
                w.appendLine("        }")
            }

            w.appendLine("    }")
            w.appendLine("}")
        }
    }
}

/**
 * Service-loaded entry point KSP uses to instantiate the processor.
 * `whisker-ksp/src/main/resources/META-INF/services/
 * com.google.devtools.ksp.processing.SymbolProcessorProvider` lists
 * this class as the discovered provider.
 */
public class WhiskerModuleProcessorProvider : SymbolProcessorProvider {
    override fun create(environment: SymbolProcessorEnvironment): SymbolProcessor =
        WhiskerModuleProcessor(
            codeGenerator = environment.codeGenerator,
            logger = environment.logger,
            moduleName = environment.options["whisker.moduleName"],
            crateName = environment.options["whisker.crateName"],
        )
}
