package rs.whisker.runtime.measure

import java.nio.ByteBuffer
import java.nio.ByteOrder
import rs.whisker.runtime.WhiskerValue

/**
 * Little-endian tagged encoding of a [WhiskerValue] tree, decoded and encoded by
 * `value_bytes` in the Android C bridge so a paragraph crosses JNI as one array.
 */
internal object ValueBytes {
    private const val NULL: Byte = 0
    private const val BOOL: Byte = 1
    private const val INT: Byte = 2
    private const val FLOAT: Byte = 3
    private const val STRING: Byte = 4
    private const val BYTES: Byte = 5
    private const val ARRAY: Byte = 6
    private const val MAP: Byte = 7
    private const val ERROR: Byte = 8

    fun encode(value: WhiskerValue): ByteArray {
        val out = Writer()
        out.value(value)
        return out.toByteArray()
    }

    fun decode(bytes: ByteArray): WhiskerValue {
        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
        val value = buffer.value()
        require(!buffer.hasRemaining()) { "trailing bytes after encoded value" }
        return value
    }

    private fun ByteBuffer.value(): WhiskerValue = when (get()) {
        NULL -> WhiskerValue.Null
        BOOL -> WhiskerValue.Bool(get() != 0.toByte())
        INT -> WhiskerValue.Int(long)
        FLOAT -> WhiskerValue.Float(double)
        STRING -> WhiskerValue.Str(string())
        BYTES -> WhiskerValue.Bytes(ByteArray(length()).also(::get))
        ARRAY -> WhiskerValue.Array(List(length()) { value() })
        MAP -> {
            val count = length()
            val entries = LinkedHashMap<String, WhiskerValue>(count * 2)
            repeat(count) { entries[string()] = value() }
            WhiskerValue.Map(entries)
        }
        ERROR -> WhiskerValue.Err(string())
        else -> throw IllegalArgumentException("unknown encoded value tag")
    }

    private fun ByteBuffer.length(): Int = int.also { require(it in 0..remaining()) }

    private fun ByteBuffer.string(): String {
        val length = length()
        val text = String(array(), arrayOffset() + position(), length, Charsets.UTF_8)
        position(position() + length)
        return text
    }

    private class Writer {
        private var bytes = ByteArray(256)
        private var size = 0

        fun toByteArray(): ByteArray = bytes.copyOf(size)

        fun value(value: WhiskerValue) {
            when (value) {
                WhiskerValue.Null -> byte(NULL)
                is WhiskerValue.Bool -> { byte(BOOL); byte(if (value.value) 1 else 0) }
                is WhiskerValue.Int -> { byte(INT); long(value.value) }
                is WhiskerValue.Float -> { byte(FLOAT); long(value.value.toRawBits()) }
                is WhiskerValue.Str -> { byte(STRING); string(value.value) }
                is WhiskerValue.Bytes -> { byte(BYTES); raw(value.value) }
                is WhiskerValue.Array -> {
                    byte(ARRAY)
                    int(value.value.size)
                    value.value.forEach(::value)
                }
                is WhiskerValue.Map -> {
                    byte(MAP)
                    int(value.value.size)
                    value.value.forEach { (key, item) -> string(key); value(item) }
                }
                is WhiskerValue.Err -> { byte(ERROR); string(value.message) }
            }
        }

        private fun string(value: String) = raw(value.toByteArray(Charsets.UTF_8))

        private fun raw(value: ByteArray) {
            int(value.size)
            reserve(value.size)
            value.copyInto(bytes, size)
            size += value.size
        }

        private fun byte(value: Byte) {
            reserve(1)
            bytes[size++] = value
        }

        private fun int(value: Int) {
            reserve(4)
            for (shift in 0 until 32 step 8) bytes[size++] = (value ushr shift).toByte()
        }

        private fun long(value: Long) {
            reserve(8)
            for (shift in 0 until 64 step 8) bytes[size++] = (value ushr shift).toByte()
        }

        private fun reserve(extra: Int) {
            if (size + extra > bytes.size) bytes = bytes.copyOf(maxOf(bytes.size * 2, size + extra))
        }
    }
}
