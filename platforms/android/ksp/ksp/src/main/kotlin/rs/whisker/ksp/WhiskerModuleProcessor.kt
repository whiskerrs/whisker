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
import com.google.devtools.ksp.symbol.KSDeclaration
import com.google.devtools.ksp.symbol.KSVisitorVoid
import com.google.devtools.ksp.symbol.Modifier

/**
 * KSP processor that scans each module subproject's compilation for
 * every concrete subclass of `rs.whisker.runtime.Module` (the
 * ModuleDefinition DSL base class) and generates a per-subproject
 * `rs.whisker.runtime.generated.<ModuleName>Behaviors` Kotlin object
 * whose `registerAll()` does the Lynx behaviour / module-registry
 * wiring.
 *
 * Discovery is **inheritance-based** — Phase M (Issue #59) dropped
 * the `@WhiskerModule` marker annotation. A Whisker module is now
 * defined by exactly one signal: `extends rs.whisker.runtime.Module`.
 * Subclassing the base class is the registration trigger; no
 * companion annotation needs to be applied at the declaration site.
 *
 * For each subclass found: instantiates it, reads its `definition()`,
 * registers a Lynx `Behavior` for view-bearing modules, and calls
 * `.registerWithLynx()`. `registerWithLynx()` branches internally —
 * view-bearing modules install their Prop / Function dispatchers
 * via the L-1 Lynx APIs; view-less modules register with
 * `WhiskerModuleRegistry` so `whisker_bridge_invoke_module` from
 * Rust routes to the DSL's `Function` handlers.
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
     * Per-subproject KSP run identifier (Phase 7-Φ.G). Passed via
     * Gradle's `ksp { arg("whisker.moduleName", "<Name>") }` in each
     * Whisker module's `build.gradle.kts`. The processor uses this
     * to name the generated file (`<ModuleName>Behaviors.kt`) and
     * the top-level Kotlin object inside it, so two modules linked
     * into the same user-app composite build don't shadow each
     * other's `registerAll()` entry point.
     *
     * `null` falls back to the legacy `WhiskerModuleBehaviors`
     * name — used by user apps that still run KSP themselves
     * (pre-Phase G).
     */
    private val moduleName: String?,
    /**
     * Cargo crate name (e.g. "whisker-hello-element"), passed via
     * Gradle's `ksp { arg("whisker.crateName", "<crate>") }` in
     * each Whisker module's `build.gradle.kts`. Used as the
     * element tag namespace so two unrelated modules' identical
     * local tag names don't collide in Lynx's behaviour registry.
     * `null` defaults to no namespace prefix (legacy behaviour).
     */
    private val crateName: String?,
) : SymbolProcessor {

    /** FQN of the base class every Whisker module must extend.
     *  Discovery is inheritance-based — extending this is the
     *  registration trigger. */
    private val moduleBaseFqn = "rs.whisker.runtime.Module"

    /**
     * KSP invokes `process` at least twice per compilation: once
     * when the user code is first processed (sources visible) and
     * again after generated code has been integrated. The `generated`
     * guard avoids double-writing the file on the second invocation.
     */
    private var generated = false

    override fun process(resolver: Resolver): List<KSAnnotated> {
        if (generated) return emptyList()

        // DSL modules. Discovery: every concrete class whose direct
        // or transitive superclass is `rs.whisker.runtime.Module`.
        // Phase M (#59) — replaces the previous `@WhiskerModule`
        // marker annotation; subclassing the base class is the sole
        // registration trigger so module authors don't have to
        // remember a companion annotation.
        val collector = ModuleSubclassCollector(moduleBaseFqn)
        resolver.getAllFiles().forEach { file ->
            file.declarations.forEach { it.accept(collector, Unit) }
        }
        val dslModuleSymbols = collector.hits

        // Always write the file, even when empty, so the user app's
        // `Application.onCreate()` call to
        // `<Module>Behaviors.registerAll()` always resolves — mirrors
        // the iOS-side `WhiskerModuleBehaviors.swift` policy.
        writeBehavioursFile(dslModuleSymbols)
        generated = true

        return emptyList()
    }

    /**
     * Visitor that collects every concrete (non-abstract) class
     * whose superclass chain reaches `moduleBaseFqn`. Recurses into
     * nested classes so a module declared inside another scope is
     * still discovered. Skips the base class itself.
     */
    private inner class ModuleSubclassCollector(
        private val baseFqn: String,
    ) : KSVisitorVoid() {
        val hits: MutableList<KSClassDeclaration> = mutableListOf()

        override fun visitClassDeclaration(
            classDeclaration: KSClassDeclaration,
            data: Unit,
        ) {
            if (
                classDeclaration.classKind == ClassKind.CLASS &&
                !classDeclaration.modifiers.contains(Modifier.ABSTRACT) &&
                classDeclaration.qualifiedName?.asString() != baseFqn &&
                extendsBase(classDeclaration)
            ) {
                hits.add(classDeclaration)
            }
            // Recurse so a module declared inside another class is
            // still found (rare but legal).
            classDeclaration.declarations.forEach { it.accept(this, Unit) }
        }

        override fun visitDeclaration(declaration: KSDeclaration, data: Unit) {
            // No-op — only KSClassDeclaration is interesting.
        }

        /**
         * Walks the superType chain until it hits `baseFqn` or runs
         * out. Uses KSP's type resolver so the match is FQN-exact
         * (no false positives from a user-defined `Module` in
         * another package).
         */
        private fun extendsBase(cls: KSClassDeclaration): Boolean {
            for (superRef in cls.superTypes) {
                val superType = superRef.resolve()
                val superDecl = superType.declaration as? KSClassDeclaration ?: continue
                if (superDecl.qualifiedName?.asString() == baseFqn) return true
                if (extendsBase(superDecl)) return true
            }
            return false
        }
    }

    private fun writeBehavioursFile(dslModules: List<KSClassDeclaration>) {
        // `Dependencies(aggregating = true, *sourceFiles)` makes the
        // generated file invalidate when ANY of the input source
        // files changes (add/remove of a `Module` subclass).
        // Important for incremental compilation — without
        // `aggregating = true` KSP wouldn't re-run when a new
        // subclass appears.
        val sourceFiles = dslModules.mapNotNull { it.containingFile }
        val dependencies = Dependencies(aggregating = true, *sourceFiles.toTypedArray())

        // File / object name. Per-subproject KSP runs (Phase G) pass
        // `whisker.moduleName` so each module's compilation produces
        // its own uniquely-named `<ModuleName>Behaviors.kt` — the
        // user app's whisker-build-generated aggregator imports each
        // and chains the per-module `registerAll()` calls. Pre-Phase
        // G fallback keeps the original `WhiskerModuleBehaviors`
        // name so user-app-level KSP still works.
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
            w.appendLine("// Sourced from `rs.whisker.runtime.Module` subclasses in this")
            w.appendLine("// Whisker module subproject. View-bearing modules register a Lynx")
            w.appendLine("// Behavior under the fully-qualified tag")
            w.appendLine("// `${crateName ?: "<no-namespace>"}:<Name>` — the namespace is the")
            w.appendLine("// cargo crate name passed via `ksp { arg(\"whisker.crateName\", \"…\") }`")
            w.appendLine("// so two modules can both declare a `Hello` element without colliding.")
            w.appendLine("//")
            w.appendLine("// Module subclass registrations: ${dslModules.size}")
            w.appendLine()
            w.appendLine("package rs.whisker.runtime.generated")
            w.appendLine()
            w.appendLine("import com.lynx.tasm.LynxEnv")
            w.appendLine("import com.lynx.tasm.behavior.Behavior")
            w.appendLine("import com.lynx.tasm.behavior.LynxContext")
            w.appendLine("import com.lynx.tasm.behavior.ui.LynxUI")
            w.appendLine("import rs.whisker.runtime.registerWithLynx")
            w.appendLine("import java.util.concurrent.atomic.AtomicBoolean")
            w.appendLine()
            w.appendLine("public object $behaviorsObjectName {")
            w.appendLine("    private val registered = AtomicBoolean(false)")
            w.appendLine()
            w.appendLine("    @JvmStatic")
            w.appendLine("    public fun registerAll() {")
            w.appendLine("        if (!registered.compareAndSet(false, true)) return")
            w.appendLine("        val env = LynxEnv.inst()")
            if (dslModules.isEmpty()) {
                w.appendLine("        // (no rs.whisker.runtime.Module subclass found)")
            }

            // ---- DSL modules ------------------------------
            //
            // For each `rs.whisker.runtime.Module` subclass:
            //   1. Build an instance.
            //   2. Read its `definitionLazy`.
            //   3. If a `View(...)` block is present, register a
            //      `Behavior` against the user's view class so Lynx
            //      can instantiate it on element creation.
            //   4. Call `.registerWithLynx()` — which installs the
            //      view's Prop / Function dispatchers (view-bearing)
            //      OR registers the module-level `Function`s with
            //      `WhiskerModuleRegistry` (view-less, Phase L-3).
            //
            // `registerWithLynx()` branches internally on whether the
            // definition has a `View(...)` block, so the codegen path
            // is identical for both shapes — we only add the
            // `addBehavior(...)` call when a view exists.
            for (cls in dslModules) {
                val fqn = cls.qualifiedName?.asString()
                if (fqn == null) {
                    logger.warn(
                        "Module subclass has no qualified name; skipping",
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
                w.appendLine("            if ($nameLocal != null) {")
                // View-bearing: register the Lynx Behavior so the tag
                // resolves to the view class. View-less modules skip
                // this — they have no element to instantiate.
                val tagPrefix = if (crateName != null) "$crateName:" else ""
                w.appendLine("                if ($viewLocal != null) {")
                w.appendLine("                    val qualifiedTag = \"$tagPrefix\" + $nameLocal")
                w.appendLine("                    val viewClass = $viewLocal.viewClass")
                // Generic reflective instantiator. The Lynx UI
                // subclass is required to expose a single-arg
                // `(LynxContext)` constructor (the
                // `WhiskerUI<View>(context)` convention).
                w.appendLine("                    env.addBehavior(object : Behavior(qualifiedTag) {")
                w.appendLine("                        override fun createUI(context: LynxContext): LynxUI<*> =")
                w.appendLine("                            viewClass.getConstructor(LynxContext::class.java)")
                w.appendLine("                                .newInstance(context) as LynxUI<*>")
                w.appendLine("                        override fun createUIFiber(context: LynxContext): LynxUI<*> =")
                w.appendLine("                            viewClass.getConstructor(LynxContext::class.java)")
                w.appendLine("                                .newInstance(context) as LynxUI<*>")
                w.appendLine("                    })")
                w.appendLine("                }")
                // Install dispatch: view Prop/Function (view-bearing)
                // or module-level Function registration (view-less,
                // keyed by `<crate>:Name`).
                w.appendLine("                // Install dispatch (view: Prop/Function; view-less: module Function).")
                val crateArg = if (crateName != null) "\"$crateName\"" else "null"
                w.appendLine("                $instanceLocal.registerWithLynx($crateArg)")
                w.appendLine("            }")
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
