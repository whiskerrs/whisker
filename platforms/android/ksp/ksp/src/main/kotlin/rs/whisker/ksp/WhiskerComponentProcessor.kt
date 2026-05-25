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
import com.google.devtools.ksp.symbol.KSFunctionDeclaration
import com.google.devtools.ksp.symbol.Modifier

/**
 * KSP processor that scans each module subproject's compilation for
 * every `@WhiskerComponent("LocalTag")`- AND `@WhiskerModule`-
 * annotated Kotlin class and generates a per-subproject
 * `rs.whisker.runtime.generated.<ModuleName>Behaviors` Kotlin
 * object whose `registerAll()` does the Lynx behaviour /
 * module-registry wiring.
 *
 * `registerAll()` does:
 *
 *  - For every `@WhiskerComponent`: `LynxEnv.inst().addBehavior(...)`
 *    — matches the Phase 7-Φ.H.2 element-registration path.
 *  - For every `@WhiskerModule` (a no-arg marker on a `Module`
 *    subclass authored with the ModuleDefinition DSL): instantiates
 *    the class, reads its `definition()`, registers a Lynx behavior
 *    for view-bearing modules, and calls `.registerWithLynx()`.
 *    `registerWithLynx()` branches internally — view-bearing modules
 *    install their Prop / Function dispatchers via the L-1 Lynx APIs;
 *    view-less modules register with `WhiskerModuleRegistry` so
 *    `whisker_bridge_invoke_module` from Rust routes to the DSL's
 *    `Function` handlers.
 *
 * The generated object's symbol matches what
 * `WhiskerApplication.onCreate()` already invokes — see
 * `crates/whisker-cng/src/templates/android/app/src/main/kotlin/
 * Application.kt`.
 */
public class WhiskerComponentProcessor(
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
     * Phase 7-Φ.H.2.
     */
    private val crateName: String?,
) : SymbolProcessor {

    /** FQN of the `@WhiskerComponent` annotation. Single source of truth. */
    private val elementAnnotationFqn = "rs.whisker.annotations.WhiskerComponent"

    /** FQN of the `@WhiskerModule` annotation — the marker on a
     *  `rs.whisker.runtime.Module` subclass that flags it for DSL
     *  registration. */
    private val moduleAnnotationFqn = "rs.whisker.annotations.WhiskerModule"

    /** FQN of the `@WhiskerProp` annotation — Phase 7-Φ.H.1. */
    private val propAnnotationFqn = "rs.whisker.annotations.WhiskerProp"

    /** FQN of the `@WhiskerUIMethod` annotation — Phase 7-Φ.H.2. */
    private val uiMethodAnnotationFqn = "rs.whisker.annotations.WhiskerUIMethod"

    /**
     * KSP invokes `process` at least twice per compilation: once
     * when the user code is first processed (annotations visible)
     * and again after generated code has been integrated (annotations
     * empty). The `generated` guard avoids double-writing the file
     * on the second invocation.
     */
    private var generated = false

    override fun process(resolver: Resolver): List<KSAnnotated> {
        if (generated) return emptyList()

        val elementSymbols = resolver
            .getSymbolsWithAnnotation(elementAnnotationFqn)
            .filterIsInstance<KSClassDeclaration>()
            .filter { it.classKind == ClassKind.CLASS }
            .toList()

        // DSL modules. Discovery: every class annotated with
        // `@WhiskerModule` (the marker on a `rs.whisker.runtime.Module`
        // subclass). The annotation is the explicit registration
        // trigger — idiomatic for Kotlin/KSP (cf. @HiltViewModel,
        // @Serializable), and an obvious entry point at the
        // declaration site.
        val dslModuleSymbols = resolver
            .getSymbolsWithAnnotation(moduleAnnotationFqn)
            .filterIsInstance<KSClassDeclaration>()
            .filter { it.classKind == ClassKind.CLASS }
            .filter { !it.modifiers.contains(Modifier.ABSTRACT) }
            .toList()

        // Always write the file, even when both sets are empty, so
        // the user app's `Application.onCreate()` call to
        // `<Module>Behaviors.registerAll()` always resolves —
        // mirrors the iOS-side `WhiskerModuleBehaviors.swift` policy.
        writeBehavioursFile(elementSymbols, dslModuleSymbols)
        generated = true

        return emptyList()
    }

    private fun writeBehavioursFile(
        elements: List<KSClassDeclaration>,
        dslModules: List<KSClassDeclaration>,
    ) {
        // `Dependencies(aggregating = true, *sourceFiles)` makes the
        // generated file invalidate when ANY of the input source
        // files changes (add/remove of @WhiskerComponent /
        // @WhiskerModule-annotated class). Important for incremental
        // compilation — without `aggregating = true` KSP wouldn't
        // re-run when a new annotated class appears.
        val sourceFiles = (elements + dslModules).mapNotNull { it.containingFile }
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
            w.appendLine("// AUTO-GENERATED by `whisker-ksp` (rs.whisker.ksp.WhiskerComponentProcessor).")
            w.appendLine("// DO NOT EDIT — applies/removes happen automatically on next compile.")
            w.appendLine("//")
            w.appendLine("// Sourced from `@WhiskerComponent(\"LocalTag\")` and")
            w.appendLine("// `@WhiskerModule(\"Name\")` applications in this Whisker")
            w.appendLine("// module subproject. Each @WhiskerComponent is registered with")
            w.appendLine("// the fully-qualified tag `${crateName ?: "<no-namespace>"}:<LocalTag>` —")
            w.appendLine("// the namespace is the cargo crate name passed via")
            w.appendLine("// `ksp { arg(\"whisker.crateName\", \"…\") }` so two modules can")
            w.appendLine("// both declare a `Hello` element without colliding.")
            w.appendLine("//")
            w.appendLine("// @WhiskerComponent registrations: ${elements.size}")
            w.appendLine("// @WhiskerModule registrations:    ${dslModules.size}")
            w.appendLine()
            w.appendLine("package rs.whisker.runtime.generated")
            w.appendLine()
            w.appendLine("import com.lynx.react.bridge.Callback")
            w.appendLine("import com.lynx.react.bridge.ReadableMap")
            w.appendLine("import com.lynx.tasm.LynxEnv")
            w.appendLine("import com.lynx.tasm.behavior.Behavior")
            w.appendLine("import com.lynx.tasm.behavior.LynxContext")
            w.appendLine("import com.lynx.tasm.behavior.LynxProp")
            w.appendLine("import com.lynx.tasm.behavior.LynxUIMethod")
            w.appendLine("import com.lynx.tasm.behavior.ui.LynxUI")
            w.appendLine("import rs.whisker.runtime.WhiskerModuleRegistry")
            w.appendLine("import rs.whisker.runtime.WhiskerValue")
            w.appendLine("import rs.whisker.runtime.registerWithLynx")
            // `WhiskerValue.fromReadableMap` is a companion @JvmStatic
            // — resolved through WhiskerValue itself, no extra import.
            // `toJavaObject` is a top-level extension function, needs
            // an explicit import.
            w.appendLine("import rs.whisker.runtime.toJavaObject")
            w.appendLine("import java.util.concurrent.atomic.AtomicBoolean")

            // Per-element bridge subclasses. Kotlin's `typealias`
            // keyword can't alias annotation types, so we can't
            // surface `@LynxProp` / `@LynxUIMethod` as their Whisker
            // counterparts directly. Instead, for every
            // @WhiskerComponent class that carries @WhiskerProp setters
            // or @WhiskerUIMethod methods, emit a `<Class>_LynxBridge`
            // subclass that:
            //
            //   - adds `@LynxProp(name = …)` wrapper methods that
            //     forward to the user's setter (Phase 7-Φ.H.1);
            //   - adds `@LynxUIMethod` wrapper methods that decode
            //     the incoming `ReadableMap` into a `List<WhiskerValue>`,
            //     invoke the user's method via `super`, then encode
            //     the returned `WhiskerValue` for Lynx's callback
            //     (Phase 7-Φ.H.2).
            //
            // The element registration further down instantiates the
            // bridge subclass rather than the user class, so Lynx's
            // reflection-based prop dispatch + UI-method dispatch
            // find the wrappers on the bridge without the module
            // author ever mentioning Lynx in their own code.
            for (cls in elements) {
                val fqn = cls.qualifiedName?.asString() ?: continue
                val simple = cls.simpleName.asString()
                val props = propMethods(cls)
                val uiMethods = uiMethodMethods(cls)
                if (props.isEmpty() && uiMethods.isEmpty()) continue

                w.appendLine()
                w.appendLine("/**")
                w.appendLine(" * @WhiskerProp + @WhiskerUIMethod forwarding bridge for")
                w.appendLine(" * `$fqn`. Generated so module authors avoid the")
                w.appendLine(" * direct `@LynxProp` / `@LynxUIMethod` imports (Kotlin")
                w.appendLine(" * doesn't allow typealiasing annotations).")
                w.appendLine(" */")
                w.appendLine("private class ${simple}_LynxBridge(context: LynxContext) : $fqn(context) {")
                for (m in props) {
                    val methodName = m.decl.simpleName.asString()
                    val propName = m.propName
                    val params = m.decl.parameters
                    // Each @WhiskerProp method takes a single value
                    // parameter — Lynx's reflection contract for
                    // prop setters. We render exactly that one
                    // parameter; multi-param setters would need
                    // adjustment but aren't supported by Lynx
                    // anyway.
                    if (params.size != 1) {
                        logger.error(
                            "@WhiskerProp methods must take exactly one parameter; " +
                                "`$fqn.$methodName` has ${params.size}",
                            m.decl,
                        )
                        continue
                    }
                    val param = params[0]
                    val paramName = param.name?.asString() ?: "value"
                    // Render the param type via KSP's resolved
                    // representation. For built-ins (`kotlin.String`,
                    // `kotlin.Int`, `kotlin.Boolean`, …) this yields
                    // a fully-qualified name that Kotlin source
                    // accepts unchanged. Generic args + nullability
                    // markers come through via `toString()`.
                    val paramTypeRendered = param.type.resolve().let { t ->
                        val base = t.declaration.qualifiedName?.asString() ?: t.toString()
                        if (t.isMarkedNullable) "$base?" else base
                    }
                    w.appendLine("    @LynxProp(name = \"$propName\")")
                    w.appendLine("    fun lynxSet_$methodName($paramName: $paramTypeRendered) {")
                    w.appendLine("        $methodName($paramName)")
                    w.appendLine("    }")
                }
                // @WhiskerUIMethod -> @LynxUIMethod forwarders. Lynx
                // calls the wrapper with the params NSDictionary
                // equivalent (`ReadableMap`) + a Callback the wrapper
                // invokes once with `(0, resultObject)` on success.
                // The user method shape is fixed to
                // `(List<WhiskerValue>) -> WhiskerValue` — matches
                // the @WhiskerModule contract on the dispatch side.
                //
                // The forwarder MUST be named exactly `$methodName`
                // (no `lynxInvoke_` prefix). Lynx Android's
                // `LynxUIMethodsCache` keys its method map by raw
                // `method.getName()` — `@LynxUIMethod` has no `name`
                // argument like `@LynxProp` does — so Rust's
                // `ElementRef::invoke("pause", …)` look-up only
                // resolves when the Kotlin method is literally called
                // `pause`. Co-existence with the inherited
                // `open fun pause(args: List<WhiskerValue>)` is fine:
                // they're parameter-disjoint Kotlin overloads.
                for (decl in uiMethods) {
                    val methodName = decl.simpleName.asString()
                    w.appendLine("    @LynxUIMethod")
                    w.appendLine("    fun $methodName(params: ReadableMap?, callback: Callback?) {")
                    w.appendLine("        val args = WhiskerValue.fromReadableMap(params)")
                    w.appendLine("        val result = super.$methodName(args)")
                    w.appendLine("        callback?.invoke(0, result.toJavaObject())")
                    w.appendLine("    }")
                }
                w.appendLine("}")
            }

            w.appendLine()
            w.appendLine("public object $behaviorsObjectName {")
            w.appendLine("    private val registered = AtomicBoolean(false)")
            w.appendLine()
            w.appendLine("    @JvmStatic")
            w.appendLine("    public fun registerAll() {")
            w.appendLine("        if (!registered.compareAndSet(false, true)) return")
            w.appendLine("        val env = LynxEnv.inst()")
            if (elements.isEmpty() && dslModules.isEmpty()) {
                w.appendLine("        // (no @WhiskerComponent / @WhiskerModule-annotated class found)")
            }

            for (cls in elements) {
                val fqn = cls.qualifiedName?.asString()
                if (fqn == null) {
                    logger.warn(
                        "@WhiskerComponent class has no qualified name; skipping",
                        cls,
                    )
                    continue
                }
                val tag = annotationStringArg(cls, elementAnnotationFqn, "tag")
                if (tag == null) {
                    logger.error(
                        "@WhiskerComponent on `$fqn` has no `tag` argument",
                        cls,
                    )
                    continue
                }
                // If the class has @WhiskerProp setters or
                // @WhiskerUIMethod methods, instantiate the bridge
                // subclass (which carries the @LynxProp /
                // @LynxUIMethod wrappers) rather than the user class.
                // The user class itself doesn't participate in
                // Lynx's reflection-based attribute / ui-method
                // dispatch, but the bridge subclass does.
                val simple = cls.simpleName.asString()
                val instantiated =
                    if (propMethods(cls).isNotEmpty() || uiMethodMethods(cls).isNotEmpty()) {
                        "${simple}_LynxBridge"
                    } else {
                        fqn
                    }
                // Namespace the Lynx tag with the cargo crate name
                // so two unrelated module packages can both declare
                // an element named `Video` without colliding. Matches
                // what the Rust-side `#[whisker::platform_component]`
                // proc macro emits via
                // `concat!(env!("CARGO_PKG_NAME"), ":", tag_local)`.
                // Phase 7-Φ.H.2.
                val qualifiedTag = if (crateName != null) "$crateName:$tag" else tag
                w.appendLine("        env.addBehavior(object : Behavior(\"$qualifiedTag\") {")
                w.appendLine("            override fun createUI(context: LynxContext): LynxUI<*> =")
                w.appendLine("                $instantiated(context)")
                w.appendLine("            override fun createUIFiber(context: LynxContext): LynxUI<*> =")
                w.appendLine("                $instantiated(context)")
                w.appendLine("        })")
            }

            // ---- @WhiskerModule DSL modules ------------------------------
            //
            // For each `WhiskerModule` subclass:
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
            if (dslModules.isNotEmpty()) {
                w.appendLine("        // ---- DSL modules (Phase L-2c / L-3) ----")
            }
            for (cls in dslModules) {
                val fqn = cls.qualifiedName?.asString()
                if (fqn == null) {
                    logger.warn(
                        "WhiskerModule subclass has no qualified name; skipping",
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
                // `(LynxContext)` constructor (matches the
                // `WhiskerUI<View>(context)` convention authors
                // already follow under the annotation API).
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
                // or module-level Function registration (view-less).
                w.appendLine("                // Install dispatch (view: Prop/Function; view-less: module Function).")
                w.appendLine("                $instanceLocal.registerWithLynx()")
                w.appendLine("            }")
                w.appendLine("        }")
            }

            w.appendLine("    }")
            w.appendLine("}")
        }
    }

    /** One discovered `@WhiskerProp("name") fun setX(...)` setter. */
    data class PropMethod(val decl: KSFunctionDeclaration, val propName: String)

    /**
     * Find every `@WhiskerProp("name")`-annotated instance method
     * on `cls`. Phase 7-Φ.H.1 — used to emit `<Class>_LynxBridge`
     * subclasses that carry the real `@LynxProp(name = …)` setters.
     *
     * Skips methods missing the `name` argument (KSP-level error
     * logged separately) so the rest of the bridge still compiles.
     */
    private fun propMethods(cls: KSClassDeclaration): List<PropMethod> {
        val out = mutableListOf<PropMethod>()
        for (decl in cls.declarations) {
            if (decl !is KSFunctionDeclaration) continue
            if (decl.simpleName.asString() == "<init>") continue
            val annotation = decl.annotations.firstOrNull {
                it.annotationType.resolve().declaration.qualifiedName?.asString() == propAnnotationFqn
            } ?: continue
            val name = annotation.arguments
                .firstOrNull { it.name?.asString() == "name" || it.name == null }
                ?.value as? String
            if (name == null) {
                logger.error(
                    "@WhiskerProp on `${cls.qualifiedName?.asString()}.${decl.simpleName.asString()}` " +
                        "has no `name` argument",
                    decl,
                )
                continue
            }
            out.add(PropMethod(decl, name))
        }
        return out
    }

    /**
     * Find every `@WhiskerUIMethod`-annotated instance method on
     * `cls`. Phase 7-Φ.H.2 — used to emit `@LynxUIMethod`-tagged
     * forwarders onto the `<Class>_LynxBridge` subclass so Lynx's
     * `LynxUIMethodsExecutor` finds them via reflection.
     *
     * The method shape is fixed to `(List<WhiskerValue>) -> WhiskerValue`
     * but we don't enforce it here (compile-time signature errors
     * surface clearly enough when the generated `super.$method(args)`
     * call doesn't typecheck). Method-side validation is downstream;
     * here we only need the names.
     */
    private fun uiMethodMethods(cls: KSClassDeclaration): List<KSFunctionDeclaration> {
        val out = mutableListOf<KSFunctionDeclaration>()
        for (decl in cls.declarations) {
            if (decl !is KSFunctionDeclaration) continue
            if (decl.simpleName.asString() == "<init>") continue
            val hasAnno = decl.annotations.any {
                it.annotationType.resolve().declaration.qualifiedName?.asString() == uiMethodAnnotationFqn
            }
            if (!hasAnno) continue
            out.add(decl)
        }
        return out
    }


    /**
     * Pull a named string argument out of the `[annotationFqn]`
     * application on `cls`. Returns `null` when no matching
     * argument is found.
     */
    private fun annotationStringArg(
        cls: KSClassDeclaration,
        annotationFqn: String,
        argName: String,
    ): String? {
        for (annotation in cls.annotations) {
            val declared = annotation.annotationType.resolve().declaration
            if (declared.qualifiedName?.asString() != annotationFqn) continue
            for (arg in annotation.arguments) {
                if (arg.name?.asString() == argName || arg.name == null) {
                    return arg.value as? String
                }
            }
        }
        return null
    }
}

/**
 * Service-loaded entry point KSP uses to instantiate the processor.
 * `whisker-ksp/src/main/resources/META-INF/services/
 * com.google.devtools.ksp.processing.SymbolProcessorProvider` lists
 * this class as the discovered provider.
 */
public class WhiskerComponentProcessorProvider : SymbolProcessorProvider {
    override fun create(environment: SymbolProcessorEnvironment): SymbolProcessor =
        WhiskerComponentProcessor(
            codeGenerator = environment.codeGenerator,
            logger = environment.logger,
            moduleName = environment.options["whisker.moduleName"],
            crateName = environment.options["whisker.crateName"],
        )
}
