package rs.whisker.runtime

import android.content.Context
import android.util.Log
import android.view.View
import java.util.concurrent.ConcurrentHashMap

public typealias WhiskerElementEventSink = (WhiskerEventBinding, WhiskerValue) -> Unit

/** Taffy available-space constraint on one measurement axis. */
public enum class WhiskerAvailableSpace { DEFINITE, MIN_CONTENT, MAX_CONTENT }

public data class WhiskerMeasureRequest(
    val availableWidth: Float?, val availableHeight: Float?,
    val availableWidthKind: WhiskerAvailableSpace,
    val availableHeightKind: WhiskerAvailableSpace,
    val knownWidth: Float?, val knownHeight: Float?,
    val payloadVersion: Int, val payload: WhiskerValue,
)

public data class WhiskerMeasuredSize(val width: Float, val height: Float)

/** Decoded `SetText` payload passed to the Host element implementation. */
public data class WhiskerTextContent(
    public val value: String,
    public val fontSize: Float,
    public val fontWeight: Int,
    public val fontFamilies: List<String> = listOf("system"),
    public val fontStyle: WhiskerFontStyle = WhiskerFontStyle.NORMAL,
    public val lineHeight: Float? = null,
    public val letterSpacing: Float = 0f,
    public val fontFeatures: List<WhiskerFontFeature> = emptyList(),
    public val fontVariations: List<WhiskerFontVariation> = emptyList(),
    public val fontOpticalSizing: WhiskerFontOpticalSizing = WhiskerFontOpticalSizing.NONE,
    public val color: Int,
    public val direction: WhiskerTextDirection = WhiskerTextDirection.AUTO,
    public val alignment: WhiskerTextAlignment = WhiskerTextAlignment.START,
    public val indent: WhiskerTextIndent = WhiskerTextIndent(),
    public val wrap: Boolean = true,
    public val wordBreak: WhiskerTextWordBreak = WhiskerTextWordBreak.NORMAL,
    public val maxLines: Int = 0,
    public val overflow: WhiskerTextOverflow = WhiskerTextOverflow.CLIP,
    public val decoration: WhiskerTextDecoration? = null,
    public val shadow: WhiskerTextShadow? = null,
)

/** Resolved inherited text style delivered independently from text content. */
public data class WhiskerTextStyle(
    public val fontSize: Float,
    public val fontWeight: Int,
    public val fontFamilies: List<String> = listOf("system"),
    public val fontStyle: WhiskerFontStyle = WhiskerFontStyle.NORMAL,
    public val lineHeight: Float? = null,
    public val letterSpacing: Float = 0f,
    public val fontFeatures: List<WhiskerFontFeature> = emptyList(),
    public val fontVariations: List<WhiskerFontVariation> = emptyList(),
    public val fontOpticalSizing: WhiskerFontOpticalSizing = WhiskerFontOpticalSizing.NONE,
    public val color: Int,
    public val direction: WhiskerTextDirection = WhiskerTextDirection.AUTO,
    public val alignment: WhiskerTextAlignment = WhiskerTextAlignment.START,
    public val decoration: WhiskerTextDecoration? = null,
    public val shadow: WhiskerTextShadow? = null,
)

public fun WhiskerTextContent.styleSnapshot(): WhiskerTextStyle = WhiskerTextStyle(
    fontSize = fontSize,
    fontWeight = fontWeight,
    fontFamilies = fontFamilies,
    fontStyle = fontStyle,
    lineHeight = lineHeight,
    letterSpacing = letterSpacing,
    fontFeatures = fontFeatures,
    fontVariations = fontVariations,
    fontOpticalSizing = fontOpticalSizing,
    color = color,
    direction = direction,
    alignment = alignment,
    decoration = decoration,
    shadow = shadow,
)

public data class WhiskerFontFeature(public val tag: String, public val value: Long)
public data class WhiskerFontVariation(public val tag: String, public val value: Float)
public enum class WhiskerFontOpticalSizing { AUTO, NONE }
public enum class WhiskerFontStyle { NORMAL, ITALIC, OBLIQUE }

public enum class WhiskerTextDirection { AUTO, LEFT_TO_RIGHT, RIGHT_TO_LEFT }

public enum class WhiskerTextAlignment { START, END, LEFT, RIGHT, CENTER }

public enum class WhiskerTextWordBreak { NORMAL, BREAK_ALL, KEEP_ALL }

public enum class WhiskerTextOverflow { CLIP, ELLIPSIS }

/** First-line indentation; percentage is relative to the final Text width. */
public data class WhiskerTextIndent(
    public val logicalPixels: Float = 0f,
    public val percentage: Float = 0f,
)

/** One inherited Whisker text decoration. */
public data class WhiskerTextDecoration(
    public val line: WhiskerTextDecorationLine,
    public val style: WhiskerTextDecorationStyle,
    public val color: Int,
)

public enum class WhiskerTextDecorationLine { UNDERLINE, LINE_THROUGH }

public enum class WhiskerTextDecorationStyle { SOLID, DOUBLE, DOTTED, DASHED, WAVY }

/** One resolved shadow painted behind native text glyphs. */
public data class WhiskerTextShadow(
    public val offsetX: Float,
    public val offsetY: Float,
    public val blurRadius: Float,
    public val color: Int,
)

/** One native View mounted for a retained Whisker node. */
public class WhiskerMountedElement internal constructor(
    public val registration: WhiskerElementRegistration,
    public val view: View,
    private val textUpdater: ((View, WhiskerTextContent) -> Unit)?,
    private val textStyleUpdater: ((View, WhiskerTextStyle) -> Unit)?,
    private val childrenHost: ((View) -> android.view.ViewGroup)?,
    private val properties: Map<Int, WhiskerPropComponent>,
    private val commands: Map<Int, WhiskerCommandComponent>,
    private val eventsByName: Map<String, WhiskerEventBinding>,
    eventSink: WhiskerElementEventSink,
) {
    private var eventMask: Long = 0
    private var eventSink: WhiskerElementEventSink = eventSink

    init {
        installEventSink()
    }

    private fun installEventSink() {
        (view as? WhiskerEventSource)?.installWhiskerEventSink { name, detail ->
            val event = eventsByName[name] ?: return@installWhiskerEventSink
            val bit = 1L shl (event.id - 1)
            if (eventMask and bit != 0L) eventSink(event, detail)
        }
    }

    public fun setProperty(id: Int, value: WhiskerValue) {
        properties[id]?.setter?.invoke(view, value)
    }

    /** Clear is a protocol operation and is intentionally not `WhiskerValue.Null`. */
    public fun clearProperty(id: Int) {
        properties[id]?.clearer?.invoke(view)
    }

    public fun invokeCommand(id: Int, parameters: WhiskerValue) {
        commands[id]?.handler?.invoke(view, parameters)
    }

    public fun setEventMask(mask: Long) { eventMask = mask }

    public fun setText(content: WhiskerTextContent): Boolean {
        val update = textUpdater ?: return false
        update(view, content)
        return true
    }

    public fun setTextStyle(style: WhiskerTextStyle): Boolean {
        val update = textStyleUpdater ?: return false
        update(view, style)
        return true
    }

    public fun childrenHost(): android.view.ViewGroup? = childrenHost?.invoke(view)

    /** Resets protocol-owned state before a built-in presentation is reused. */
    public fun prepareForReuse(eventSink: WhiskerElementEventSink) {
        properties.values.forEach { it.clearer(view) }
        eventMask = 0
        this.eventSink = eventSink
        installEventSink()
    }

    public fun dispose() { (view as? WhiskerEventSource)?.installWhiskerEventSink(null) }
}

/** Host-owned declaration. It contains names and behavior, never Rust IDs. */
public class WhiskerElementFactory(
    public val name: String,
    internal val textUpdater: ((View, WhiskerTextContent) -> Unit)? = null,
    internal val textStyleUpdater: ((View, WhiskerTextStyle) -> Unit)? = null,
    internal val childrenHost: ((View) -> android.view.ViewGroup)? = null,
    internal val measurer: ((WhiskerMeasureRequest) -> WhiskerMeasuredSize?)? = null,
    internal val makeView: (Context) -> View,
) {
    init {
        require(name.isNotEmpty() && '@' !in name) {
            "Host element name must be non-empty and versionless"
        }
    }


    internal fun withTextStyleUpdater(
        updater: ((View, WhiskerTextStyle) -> Unit)?,
    ): WhiskerElementFactory = WhiskerElementFactory(
        name = name,
        textUpdater = textUpdater,
        textStyleUpdater = updater ?: textStyleUpdater,
        childrenHost = childrenHost,
        measurer = measurer,
        makeView = makeView,
    )

    internal fun withMeasurer(
        provider: ((WhiskerMeasureRequest) -> WhiskerMeasuredSize?)?,
    ): WhiskerElementFactory = WhiskerElementFactory(
        name = name,
        textUpdater = textUpdater,
        textStyleUpdater = textStyleUpdater,
        childrenHost = childrenHost,
        measurer = provider ?: measurer,
        makeView = makeView,
    )
}

private data class WhiskerDeclaredElement(
    val factory: WhiskerElementFactory,
    val properties: Map<String, WhiskerPropComponent>,
    val events: Set<String>,
    val commands: Map<String, WhiskerCommandComponent>,
)

internal data class WhiskerBoundElement(
    val registration: WhiskerElementRegistration,
    val factory: WhiskerElementFactory,
    val properties: Map<Int, WhiskerPropComponent>,
    val commands: Map<Int, WhiskerCommandComponent>,
)

/** One surface's immutable-after-bootstrap mapping from Rust IDs to Host declarations. */
public class WhiskerElementBindings private constructor() {
    @Volatile private var boundByType: Map<Int, WhiskerBoundElement> = emptyMap()

    internal fun publish(bindings: Map<Int, WhiskerBoundElement>) {
        boundByType = bindings
    }

    public fun mount(
        elementType: Int,
        context: Context,
        eventSink: WhiskerElementEventSink,
    ): WhiskerMountedElement? {
        val element = boundByType[elementType] ?: return null
        val view = element.factory.makeView(context)
        return WhiskerMountedElement(
            element.registration,
            view,
            element.factory.textUpdater,
            element.factory.textStyleUpdater,
            element.factory.childrenHost,
            element.properties,
            element.commands,
            element.registration.events.associateBy { it.name },
            eventSink,
        )
    }

    public fun measure(elementType: Int, request: WhiskerMeasureRequest): WhiskerMeasuredSize? =
        boundByType[elementType]?.factory?.measurer?.invoke(request)

    public fun registration(elementType: Int): WhiskerElementRegistration? =
        boundByType[elementType]?.registration

    internal companion object {
        fun empty(): WhiskerElementBindings = WhiskerElementBindings()
    }
}

/** Process-wide Host declarations used to negotiate independent surface-local binding tables. */
public object WhiskerElementRegistry {
    private const val LOG_TAG = "WhiskerElementRegistry"
    private val declarations = ConcurrentHashMap<String, WhiskerDeclaredElement>()

    @JvmStatic
    public fun newBindings(): WhiskerElementBindings = WhiskerElementBindings.empty()

    @JvmStatic
    public fun register(factory: WhiskerElementFactory) {
        register(factory, emptyMap(), emptySet(), emptyMap())
    }

    internal fun register(view: WhiskerViewComponent, fallbackName: String) {
        val name = view.elementName ?: fallbackName
        view.factory?.let { factory ->
            val properties = view.components.filterIsInstance<WhiskerPropComponent>().associateBy { it.name }
            val events = view.components.filterIsInstance<WhiskerEventsComponent>().flatMap { it.names }.toSet()
            val commands = view.components.filterIsInstance<WhiskerCommandComponent>().associateBy { it.name }
            val textStyle = view.components.filterIsInstance<WhiskerTextStyleComponent>().singleOrNull()
            val measurement = view.components.filterIsInstance<WhiskerMeasurementComponent>().singleOrNull()
            register(
                factory.withTextStyleUpdater(textStyle?.handler).withMeasurer(measurement?.handler),
                properties,
                events,
                commands,
            )
            return
        }
        val declaredClass = requireNotNull(view.viewClass) { "$name View declaration needs a class or factory" }
        require(View::class.java.isAssignableFrom(declaredClass)) {
            "$name View class must extend android.view.View"
        }
        @Suppress("UNCHECKED_CAST")
        val viewClass = declaredClass as Class<out View>
        val properties = view.components.filterIsInstance<WhiskerPropComponent>().associateBy { it.name }
        val events = view.components.filterIsInstance<WhiskerEventsComponent>().flatMap { it.names }.toSet()
        val commands = view.components.filterIsInstance<WhiskerCommandComponent>().associateBy { it.name }
        val textStyle = view.components.filterIsInstance<WhiskerTextStyleComponent>().singleOrNull()
        val measurement = view.components.filterIsInstance<WhiskerMeasurementComponent>().singleOrNull()
        register(
            WhiskerElementFactory(
                name = name,
                textStyleUpdater = textStyle?.handler,
                measurer = measurement?.handler,
                makeView = { context -> viewClass.getConstructor(Context::class.java).newInstance(context) },
            ),
            properties,
            events,
            commands,
        )
    }

    private fun register(
        factory: WhiskerElementFactory,
        properties: Map<String, WhiskerPropComponent>,
        events: Set<String>,
        commands: Map<String, WhiskerCommandComponent>,
    ) {
        require(declarations.putIfAbsent(factory.name, WhiskerDeclaredElement(factory, properties, events, commands)) == null) {
            "element factory already registered for ${factory.name}"
        }
    }

    /** Match Host strings to Rust registrations and compile compact dispatch tables. */
    @JvmStatic
    public fun bind(
        target: WhiskerElementBindings,
        registrations: List<WhiskerElementRegistration>,
    ): Boolean {
        fun reject(message: String): Boolean {
            Log.e(LOG_TAG, "Host element bootstrap rejected: $message")
            return false
        }

        val byType = LinkedHashMap<Int, WhiskerBoundElement>()
        val byName = LinkedHashMap<String, WhiskerBoundElement>()
        for (registration in registrations) {
            val declaration = declarations[registration.name]
                ?: return reject("no Host declaration for `${registration.name}`")
            if ((registration.childPolicy == WhiskerChildPolicy.PlainText) != (declaration.factory.textUpdater != null)) {
                return reject("child policy mismatch for `${registration.name}`: Rust=${registration.childPolicy}, Host text updater=${declaration.factory.textUpdater != null}")
            }
            if (registration.textStyle != (declaration.factory.textStyleUpdater != null)) {
                return reject("text-style capability mismatch for `${registration.name}`: Rust=${registration.textStyle}, Host=${declaration.factory.textStyleUpdater != null}")
            }
            val needsHostMeasurer = registration.measurement == WhiskerMeasurement.ReplacedContent ||
                registration.measurement == WhiskerMeasurement.Custom
            if (needsHostMeasurer != (declaration.factory.measurer != null)) {
                return reject("measurement capability mismatch for `${registration.name}`: Rust=${registration.measurement}, Host measurer=${declaration.factory.measurer != null}")
            }
            val rustProps = registration.properties.associateBy { it.name }
            if (rustProps.keys != declaration.properties.keys) {
                return reject("property mismatch for `${registration.name}`: Rust=${rustProps.keys.sorted()}, Host=${declaration.properties.keys.sorted()}")
            }
            val rustEvents = registration.events.map { it.name }.toSet()
            if (rustEvents != declaration.events) {
                return reject("event mismatch for `${registration.name}`: Rust=${rustEvents.sorted()}, Host=${declaration.events.sorted()}")
            }
            val rustCommands = registration.commands.map { it.name }.toSet()
            if (rustCommands != declaration.commands.keys) {
                return reject("command mismatch for `${registration.name}`: Rust=${rustCommands.sorted()}, Host=${declaration.commands.keys.sorted()}")
            }
            val properties = registration.properties.associate { property ->
                property.id to requireNotNull(declaration.properties[property.name])
            }
            val commands = registration.commands.associate { command ->
                command.id to requireNotNull(declaration.commands[command.name])
            }
            val bound = WhiskerBoundElement(registration, declaration.factory, properties, commands)
            if (byType.put(registration.elementType, bound) != null) {
                return reject("duplicate Rust element type ${registration.elementType}")
            }
            if (byName.put(registration.name, bound) != null) {
                return reject("duplicate Rust element name `${registration.name}`")
            }
        }
        target.publish(byType)
        return true
    }
}

/** Single bootstrap boundary for independently compiled Host modules. */
public object WhiskerModuleKernel {
    private val installedNames = ConcurrentHashMap.newKeySet<String>()

    /** Validates and installs one complete service + element declaration. */
    @JvmStatic
    public fun install(module: Module, crateName: String? = null) {
        val def = module.definitionLazy
        def.validateElementDeclaration()
        val name = requireNotNull(def.name) { "ModuleDefinition requires Name" }
        val qualifiedName = module.qualifiedName
            ?: if (crateName.isNullOrEmpty() || '/' in name) name else "$crateName:$name"
        require(installedNames.add(qualifiedName)) { "module already installed: $qualifiedName" }
        module.qualifiedName = qualifiedName
        WhiskerModuleEventCenter.register(module)

        def.views.forEach { WhiskerElementRegistry.register(it, qualifiedName) }

        val functions = def.functions.associateBy { it.name }
        if (functions.isNotEmpty()) {
            WhiskerModuleRegistry.registerDispatch(qualifiedName) { method, args ->
                val function = functions[method]
                    ?: return@registerDispatch WhiskerValue.Err("unknown method `$method` on module `$name`")
                function.handler(args.asList())
            }
        }
        val asyncFunctions = def.asyncFunctions.associateBy { it.name }
        if (asyncFunctions.isNotEmpty()) {
            WhiskerModuleRegistry.registerDispatchAsync(qualifiedName) { method, args, promise ->
                val function = asyncFunctions[method] ?: return@registerDispatchAsync false
                function.handler(args.asList(), promise)
                true
            }
        }
    }
}
