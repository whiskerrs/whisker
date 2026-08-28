package rs.whisker.runtime

/**
 * WhiskerValue — Kotlin mirror of the Rust `whisker::platform_module::
 * WhiskerValue` tagged union. Used by `Module`-subclass methods as
 * the universal arg/return type.
 *
 * The platform_module bridge exchanges typed Whisker values directly,
 * so author code pattern-matches on sealed-class subtypes
 * (`when (arg) { is WhiskerValue.Str -> ... }`) instead of `as?`
 * casts against `String` / `Long` / etc. — fewer silent `null`
 * results, and exhaustive `when` coverage.
 *
 * The C bridge (whisker_bridge_android.cc) constructs these
 * instances via JNI reflection on every `invoke_module` call, and
 * walks the returned subtype to build a `WhiskerValueRaw` for the
 * Rust side.
 *
 * Variants are `data class`es so equality + hashCode are structurally
 * defined.
 */
public sealed class WhiskerValue {
    public object Null : WhiskerValue() {
        override fun toString(): String = "Null"
    }

    public data class Bool(val value: Boolean) : WhiskerValue()
    public data class Int(val value: Long) : WhiskerValue()
    public data class Float(val value: Double) : WhiskerValue()
    public data class Str(val value: String) : WhiskerValue()
    public data class Bytes(val value: ByteArray) : WhiskerValue() {
        // ByteArray's default equals is referential; override so two
        // instances with the same bytes compare equal (matches the
        // other data classes' structural equality).
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is Bytes) return false
            return value.contentEquals(other.value)
        }
        override fun hashCode(): kotlin.Int = value.contentHashCode()
    }
    public data class Array(val value: List<WhiskerValue>) : WhiskerValue()
    public data class Map(val value: kotlin.collections.Map<String, WhiskerValue>) : WhiskerValue()
    public data class Err(val message: String) : WhiskerValue()

    // Typed reads for module authors destructuring raw `WhiskerValue`
    // args (`args[0].asDouble()`, `value.asString()`). Numeric reads
    // coerce between Int/Float; mismatches return null.

    public fun asString(): String? = (this as? Str)?.value
    public fun asBool(): Boolean? = (this as? Bool)?.value
    public fun asInt(): Long? = when (this) {
        is Int -> value
        is Float -> value.toLong()
        else -> null
    }
    public fun asDouble(): Double? = when (this) {
        is Float -> value
        is Int -> value.toDouble()
        else -> null
    }
    public fun asBytes(): ByteArray? = (this as? Bytes)?.value

    /** Whether this value is transferable application data rather than a call failure. */
    public fun isData(): Boolean = when (this) {
        is Array -> value.all { it.isData() }
        is Map -> value.values.all { it.isData() }
        is Err -> false
        else -> true
    }

    public companion object {
        /**
         * Convert a Java Object[] (from the JNI bridge) into a
         * Kotlin `Array<WhiskerValue>`. The C bridge has already
         * constructed each element as a `WhiskerValue` subtype
         * instance — this just casts and lifts into the typed
         * Kotlin array.
         */
        @JvmStatic
        public fun fromJavaArray(args: kotlin.Array<Any?>): kotlin.Array<WhiskerValue> {
            return kotlin.Array(args.size) { i ->
                args[i] as? WhiskerValue ?: Err("non-WhiskerValue arg at $i")
            }
        }

    }
}

/**
 * Encode a [WhiskerValue] into a Java-compatible nested
 * map/list/primitive tree suitable for handing to the JNI Host adapter.
 *
 * `Bytes` is emitted as a `ByteArray` (passes through JNI as
 * `byte[]`). `Err` becomes a `mapOf("error" to message)` since
 * `Callback` already has the error-code channel for the failure
 * signal.
 */
public fun WhiskerValue.toJavaObject(): Any? = when (this) {
    is WhiskerValue.Null -> null
    is WhiskerValue.Bool -> value
    is WhiskerValue.Int -> value
    is WhiskerValue.Float -> value
    is WhiskerValue.Str -> value
    is WhiskerValue.Bytes -> value
    is WhiskerValue.Array -> value.map { it.toJavaObject() }
    is WhiskerValue.Map -> value.mapValues { (_, v) -> v.toJavaObject() }
    is WhiskerValue.Err -> mapOf("error" to message)
}

/**
 * Wrap a raw Kotlin value (a DSL `Function` handler's return value,
 * or a positional arg already unwrapped via [toJavaObject]) back
 * into a [WhiskerValue]. Inverse of [toJavaObject] for the scalar
 * + container cases. `Unit` / `null` collapse to [WhiskerValue.Null]
 * — the convention the Rust side lifts into `()` / `Option::None`.
 * A value that's already a [WhiskerValue] passes through unchanged
 * (lets handlers return a `WhiskerValue` directly when they want
 * full control, e.g. an explicit `Err`).
 */
public fun whiskerValueOf(v: Any?): WhiskerValue = when (v) {
    null, Unit -> WhiskerValue.Null
    is WhiskerValue -> v
    is Boolean -> WhiskerValue.Bool(v)
    is kotlin.Int -> WhiskerValue.Int(v.toLong())
    is Long -> WhiskerValue.Int(v)
    is Float -> WhiskerValue.Float(v.toDouble())
    is Double -> WhiskerValue.Float(v)
    is String -> WhiskerValue.Str(v)
    is ByteArray -> WhiskerValue.Bytes(v)
    is List<*> -> WhiskerValue.Array(v.map { whiskerValueOf(it) })
    is Map<*, *> -> WhiskerValue.Map(
        v.entries.associate { (k, value) -> k.toString() to whiskerValueOf(value) },
    )
    else -> WhiskerValue.Err("unsupported return type ${v::class.java.name}")
}
