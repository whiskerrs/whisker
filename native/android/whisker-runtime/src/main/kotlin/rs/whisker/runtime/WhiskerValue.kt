package rs.whisker.runtime

/**
 * WhiskerValue — Kotlin mirror of the Rust `whisker::native_module::
 * WhiskerValue` tagged union. Used by `@WhiskerModule`-annotated
 * classes' methods as the universal arg/return type, replacing the
 * previous `Array<Any?>` + boxed-Java-type marshalling.
 *
 * Phase 7-Φ.F: the native_module bridge now exchanges typed Whisker
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
    }
}
