package rs.whisker.runtime.measure

/** Result storage returned across the Android Host measurement JNI seam. */
class HostMeasureBatchResponse(
    @JvmField val longs: LongArray,
    @JvmField val ints: IntArray,
    @JvmField val floats: FloatArray,
)

/** Flat-array layout shared with the Android C bridge. */
internal object HostMeasureBatchAbi {
    const val REQUEST_LONG_STRIDE = 3
    const val KEY = 0
    const val NODE = 1
    const val ENVIRONMENT_EPOCH = 2

    const val REQUEST_INT_STRIDE = 17
    const val ELEMENT_TYPE = 0
    const val KIND = 1
    const val KNOWN_MASK = 2
    const val AVAILABLE_WIDTH_KIND = 3
    const val AVAILABLE_HEIGHT_KIND = 4
    const val FONT_WEIGHT = 5
    const val FONT_STYLE = 6
    const val WRAP = 7
    const val WORD_BREAK = 8
    const val OVERFLOW = 9
    const val MAX_LINES = 10
    const val FONT_FEATURE_COUNT = 11
    const val FONT_OPTICAL_SIZING = 12
    const val PAYLOAD_VERSION = 13
    const val INTRINSIC_MASK = 14
    const val DIRECTION = 15
    const val ALIGNMENT = 16

    const val REQUEST_FLOAT_STRIDE = 11
    const val KNOWN_WIDTH = 0
    const val KNOWN_HEIGHT = 1
    const val AVAILABLE_WIDTH = 2
    const val AVAILABLE_HEIGHT = 3
    const val FONT_SIZE = 4
    const val LINE_HEIGHT = 5
    const val LETTER_SPACING = 6
    const val INDENT_LOGICAL_PIXELS = 7
    const val INDENT_PERCENTAGE = 8
    const val INTRINSIC_WIDTH = 9
    const val INTRINSIC_HEIGHT = 10

    const val REQUEST_STRING_STRIDE = 2
    const val TEXT = 0
    const val LOCALE = 1

    const val RESPONSE_LONG_STRIDE = 4
    const val RESPONSE_INT_STRIDE = 3
    const val RESPONSE_FLOAT_STRIDE = 4

    @Suppress("LongParameterList")
    fun measure(
        provider: HostMeasurementProvider,
        requestLongs: LongArray,
        requestInts: IntArray,
        requestFloats: FloatArray,
        requestStrings: Array<String>,
        fontFamilies: Array<Array<String>>,
        fontSettings: Array<Array<String>>,
        payloads: Array<ByteArray>,
    ): HostMeasureBatchResponse {
        require(requestLongs.size % REQUEST_LONG_STRIDE == 0)
        val count = requestLongs.size / REQUEST_LONG_STRIDE
        require(requestInts.size == count * REQUEST_INT_STRIDE)
        require(requestFloats.size == count * REQUEST_FLOAT_STRIDE)
        require(requestStrings.size == count * REQUEST_STRING_STRIDE)
        require(fontFamilies.size == count)
        require(fontSettings.size == count)
        require(payloads.size == count)

        val responseLongs = LongArray(count * RESPONSE_LONG_STRIDE)
        val responseInts = IntArray(count * RESPONSE_INT_STRIDE)
        val responseFloats = FloatArray(count * RESPONSE_FLOAT_STRIDE)
        repeat(count) { index ->
            val longBase = index * REQUEST_LONG_STRIDE
            val intBase = index * REQUEST_INT_STRIDE
            val floatBase = index * REQUEST_FLOAT_STRIDE
            val stringBase = index * REQUEST_STRING_STRIDE
            // Node remains in the batch contract for request identity even though
            // the Android measurement provider does not currently consume it.
            val measured = provider.measure(
                requestInts[intBase + ELEMENT_TYPE],
                requestInts[intBase + KIND],
                requestFloats[floatBase + KNOWN_WIDTH],
                requestFloats[floatBase + KNOWN_HEIGHT],
                requestInts[intBase + KNOWN_MASK],
                requestFloats[floatBase + AVAILABLE_WIDTH],
                requestFloats[floatBase + AVAILABLE_HEIGHT],
                requestInts[intBase + AVAILABLE_WIDTH_KIND],
                requestInts[intBase + AVAILABLE_HEIGHT_KIND],
                requestStrings[stringBase + TEXT],
                requestStrings[stringBase + LOCALE],
                fontFamilies[index],
                requestFloats[floatBase + FONT_SIZE],
                requestInts[intBase + FONT_WEIGHT],
                requestInts[intBase + FONT_STYLE],
                requestInts[intBase + WRAP],
                requestInts[intBase + WORD_BREAK],
                requestInts[intBase + OVERFLOW],
                requestFloats[floatBase + LETTER_SPACING],
                requestFloats[floatBase + LINE_HEIGHT],
                requestFloats[floatBase + INDENT_LOGICAL_PIXELS],
                requestFloats[floatBase + INDENT_PERCENTAGE],
                requestInts[intBase + MAX_LINES],
                fontSettings[index],
                requestInts[intBase + FONT_FEATURE_COUNT],
                requestInts[intBase + FONT_OPTICAL_SIZING],
                requestInts[intBase + PAYLOAD_VERSION],
                payloads[index],
                requestFloats[floatBase + INTRINSIC_WIDTH],
                requestFloats[floatBase + INTRINSIC_HEIGHT],
                requestInts[intBase + INTRINSIC_MASK],
                requestInts[intBase + DIRECTION],
                requestInts[intBase + ALIGNMENT],
            )
            require(measured.size >= 7)

            val responseLongBase = index * RESPONSE_LONG_STRIDE
            responseLongs[responseLongBase] = requestLongs[longBase + KEY]
            responseLongs[responseLongBase + 1] = requestLongs[longBase + ENVIRONMENT_EPOCH]
            // Android measurement does not currently produce prepared-content handles.
            responseLongs[responseLongBase + 2] = 0
            responseLongs[responseLongBase + 3] = 0

            val responseIntBase = index * RESPONSE_INT_STRIDE
            responseInts[responseIntBase] = measured[0].toInt()
            responseInts[responseIntBase + 1] = measured[1].toInt()
            responseInts[responseIntBase + 2] = measured[6].toInt()

            val responseFloatBase = index * RESPONSE_FLOAT_STRIDE
            responseFloats[responseFloatBase] = measured[2]
            responseFloats[responseFloatBase + 1] = measured[3]
            responseFloats[responseFloatBase + 2] = measured[4]
            responseFloats[responseFloatBase + 3] = measured[5]
        }
        return HostMeasureBatchResponse(responseLongs, responseInts, responseFloats)
    }
}
