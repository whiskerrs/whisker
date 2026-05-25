package rs.whisker.runtime

import com.lynx.react.bridge.ReadableArray
import com.lynx.react.bridge.ReadableMap
import com.lynx.react.bridge.ReadableType

/**
 * WhiskerValue — Kotlin mirror of the Rust `whisker::platform_module::
 * WhiskerValue` tagged union. Used by `@WhiskerModule`-annotated
 * classes' methods as the universal arg/return type, replacing the
 * previous `Array<Any?>` + boxed-Java-type marshalling.
 *
 * Phase 7-Φ.F: the platform_module bridge now exchanges typed Whisker
 * values directly. Author code pattern-matches on sealed-class
 * subtypes (`when (arg) { is WhiskerValue.Str -> ... }`) instead of
 * `as?` casts against `String` / `Long` / etc. — fewer silent
 * `null` results and exhaustive `when` coverage.
 *
 * The C bridge (whisker_bridge_android.cc) constructs these
 * instances via JNI reflection on every `invoke_module` call, and
 * walks the returned subtype to build a `WhiskerValueRaw` for the
 * Rust side.
 *
 * `data class` for the variants so equality + hashCode are
 * structurally defined (helps snapshot tests + log diffs).
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

        /**
         * Decode a Lynx `ReadableMap` (the `params` argument
         * `LynxUIMethodsExecutor` hands the `@LynxUIMethod` forwarder)
         * into the `List<WhiskerValue>` shape `@WhiskerUIMethod`
         * authors expect.
         *
         * Convention (Phase 7-Φ.H.2): the C bridge packs Rust-side
         * `&[WhiskerValue]` into `{"args": [...]}` — single key
         * `args` holding a `ReadableArray` of positional entries.
         * Each entry decodes via [readableValue] (recursive). Missing
         * key or non-array shape yields an empty list rather than
         * an error so the user method still runs (with no args).
         *
         * The C bridge implementation that produces this shape lives
         * in `whisker_bridge_invoke_element_method` (Phase 7-Φ.H.2.5,
         * tracked separately) — until that lands, callers receive an
         * empty list.
         */
        @JvmStatic
        public fun fromReadableMap(params: ReadableMap?): List<WhiskerValue> {
            if (params == null || !params.hasKey("args")) return emptyList()
            val array = params.getArray("args") ?: return emptyList()
            return List(array.size()) { i -> readableArrayValue(array, i) }
        }

        private fun readableArrayValue(arr: ReadableArray, i: kotlin.Int): WhiskerValue {
            return when (arr.getType(i)) {
                ReadableType.Null -> Null
                ReadableType.Boolean -> Bool(arr.getBoolean(i))
                ReadableType.Int -> Int(arr.getInt(i).toLong())
                ReadableType.Long -> Int(arr.getLong(i))
                ReadableType.Number -> Float(arr.getDouble(i))
                ReadableType.String -> Str(arr.getString(i) ?: "")
                ReadableType.Map -> Map(readableMapToMap(arr.getMap(i)))
                ReadableType.Array -> Array(readableArrayToList(arr.getArray(i)))
                ReadableType.ByteArray -> Bytes(arr.getByteArray(i) ?: ByteArray(0))
                // ReadableType has additional variants for Lynx-
                // internal payloads (PiperData / LynxObject /
                // ByteBuffer / TemplateData). They never appear in
                // user-emitted method-args payloads, but the `when`
                // needs an exhaustive branch.
                else -> Err("unsupported ReadableType in @WhiskerUIMethod args")
            }
        }

        private fun readableMapToMap(map: ReadableMap?): kotlin.collections.Map<String, WhiskerValue> {
            if (map == null) return emptyMap()
            val out = LinkedHashMap<String, WhiskerValue>()
            val iter = map.keySetIterator()
            while (iter.hasNextKey()) {
                val key = iter.nextKey()
                out[key] = when (map.getType(key)) {
                    ReadableType.Null -> Null
                    ReadableType.Boolean -> Bool(map.getBoolean(key))
                    ReadableType.Int -> Int(map.getInt(key).toLong())
                    ReadableType.Long -> Int(map.getLong(key))
                    ReadableType.Number -> Float(map.getDouble(key))
                    ReadableType.String -> Str(map.getString(key) ?: "")
                    ReadableType.Map -> Map(readableMapToMap(map.getMap(key)))
                    ReadableType.Array -> Array(readableArrayToList(map.getArray(key)))
                    ReadableType.ByteArray -> Bytes(map.getByteArray(key) ?: ByteArray(0))
                    else -> Err("unsupported ReadableType for key $key")
                }
            }
            return out
        }

        private fun readableArrayToList(arr: ReadableArray?): List<WhiskerValue> {
            if (arr == null) return emptyList()
            return List(arr.size()) { i -> readableArrayValue(arr, i) }
        }
    }
}

/**
 * Encode a [WhiskerValue] into a Java-compatible nested
 * map/list/primitive tree suitable for handing to Lynx's
 * `Callback.invoke(code, result)` (Phase 7-Φ.H.2). The host JS /
 * bridge then sees the value via Lynx's standard `Callback` -> JNI
 * marshalling.
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
