package rs.whisker.runtime

/** Intrinsic-measurement policy negotiated with the Rust runtime. */
public enum class WhiskerMeasurement { None, Text, ReplacedContent, Custom }

/** Rust-owned child semantics. Native mount targets remain Host-local. */
public enum class WhiskerChildPolicy { None, Elements, PlainText }

/** Top-level shape of a value carried by [WhiskerValue]. */
public enum class WhiskerValueKind { Null, Bool, Int, Float, String, Bytes, Array, Map }

/** Runtime-assigned property identity received during surface bootstrap. */
public data class WhiskerPropertyBinding(
    public val id: Int,
    public val name: String,
    public val value: WhiskerValueKind,
) {
    init {
        require(id > 0) { "property id must be non-zero" }
        require(name.isNotEmpty()) { "property name must not be empty" }
    }
}

/** Runtime-assigned event identity received during surface bootstrap. */
public data class WhiskerEventBinding(
    public val id: Int,
    public val name: String,
    public val detail: WhiskerValueKind? = null,
) {
    init {
        require(id > 0) { "event id must be non-zero" }
        require(name.isNotEmpty()) { "event name must not be empty" }
    }
}

/** Runtime-assigned command identity received during surface bootstrap. */
public data class WhiskerCommandBinding(
    public val id: Int,
    public val name: String,
    public val arguments: WhiskerValueKind,
) {
    init {
        require(id > 0) { "command id must be non-zero" }
        require(name.isNotEmpty()) { "command name must not be empty" }
    }
}

/**
 * Rust-owned element registration decoded from a snapshot frame.
 *
 * This is runtime negotiation data, not generated Kotlin source. Host module
 * authors declare names in `ModuleDefinition`; the registry resolves those
 * names to these compact IDs once, before applying frame operations.
 */
public class WhiskerElementRegistration(
    public val elementType: Int,
    public val name: String,
    public val childPolicy: WhiskerChildPolicy,
    public val measurement: WhiskerMeasurement,
    public val properties: List<WhiskerPropertyBinding> = emptyList(),
    public val events: List<WhiskerEventBinding> = emptyList(),
    public val commands: List<WhiskerCommandBinding> = emptyList(),
) {
    private val propertiesById = properties.associateBy { it.id }
    private val eventsById = events.associateBy { it.id }
    private val commandsById = commands.associateBy { it.id }

    init {
        require(elementType > 0) { "element type must be non-zero" }
        require(name.isNotEmpty() && '@' !in name) {
            "element name must be non-empty and versionless"
        }
        require(propertiesById.size == properties.size) { "duplicate property id in $name" }
        require(properties.map { it.name }.toSet().size == properties.size) {
            "duplicate property name in $name"
        }
        require(eventsById.size == events.size) { "duplicate event id in $name" }
        require(events.map { it.name }.toSet().size == events.size) {
            "duplicate event name in $name"
        }
        require(commandsById.size == commands.size) { "duplicate command id in $name" }
        require(commands.map { it.name }.toSet().size == commands.size) {
            "duplicate command name in $name"
        }
    }

    public fun property(id: Int): WhiskerPropertyBinding =
        requireNotNull(propertiesById[id]) { "unknown property id $id on $name" }

    public fun event(id: Int): WhiskerEventBinding =
        requireNotNull(eventsById[id]) { "unknown event id $id on $name" }

    public fun command(id: Int): WhiskerCommandBinding =
        requireNotNull(commandsById[id]) { "unknown command id $id on $name" }
}
