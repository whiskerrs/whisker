package rs.whisker.runtime.measure

import rs.whisker.runtime.WhiskerValue

/** Result storage returned across the Android Host measurement JNI seam. */
class HostMeasureBatchResponse(
    @JvmField val longs: LongArray,
    @JvmField val ints: IntArray,
    @JvmField val floats: FloatArray,
    @JvmField val paragraphs: Array<WhiskerValue?>,
    @JvmField val layouts: Array<android.text.StaticLayout?> = arrayOfNulls(paragraphs.size),
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
        paragraphs: Array<WhiskerValue?> = arrayOfNulls(payloads.size),
    ): HostMeasureBatchResponse {
        require(requestLongs.size % REQUEST_LONG_STRIDE == 0)
        val count = requestLongs.size / REQUEST_LONG_STRIDE
        require(requestInts.size == count * REQUEST_INT_STRIDE)
        require(requestFloats.size == count * REQUEST_FLOAT_STRIDE)
        require(requestStrings.size == count * REQUEST_STRING_STRIDE)
        require(fontFamilies.size == count)
        require(fontSettings.size == count)
        require(payloads.size == count)
        require(paragraphs.size == count)

        val responseLongs = LongArray(count * RESPONSE_LONG_STRIDE)
        val responseInts = IntArray(count * RESPONSE_INT_STRIDE)
        val responseFloats = FloatArray(count * RESPONSE_FLOAT_STRIDE)
        val responseParagraphs = arrayOfNulls<WhiskerValue>(count)
        val responseLayouts = arrayOfNulls<android.text.StaticLayout>(count)
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
                paragraphs[index],
            )
            responseParagraphs[index] = measured.paragraph
            responseLayouts[index] = measured.layout
            measured.layout?.let { provider.preparedParagraphs.put(requestLongs[longBase + KEY], it) }

            val responseLongBase = index * RESPONSE_LONG_STRIDE
            responseLongs[responseLongBase] = requestLongs[longBase + KEY]
            responseLongs[responseLongBase + 1] = requestLongs[longBase + ENVIRONMENT_EPOCH]
            responseLongs[responseLongBase + 2] = 0
            val prepared = requestInts[intBase + KIND] == rs.whisker.runtime.bridge.MobileAbi.MEASURE_TEXT && measured.status == rs.whisker.runtime.bridge.MobileAbi.MEASURE_READY
            responseLongs[responseLongBase + 3] = if (prepared) requestLongs[longBase + KEY] else 0

            val responseIntBase = index * RESPONSE_INT_STRIDE
            responseInts[responseIntBase] = measured.status
            responseInts[responseIntBase + 1] = measured.reason
            responseInts[responseIntBase + 2] = measured.mask or (if (prepared) 4 else 0)

            val responseFloatBase = index * RESPONSE_FLOAT_STRIDE
            responseFloats[responseFloatBase] = measured.width
            responseFloats[responseFloatBase + 1] = measured.height
            responseFloats[responseFloatBase + 2] = measured.firstBaseline
            responseFloats[responseFloatBase + 3] = measured.lastBaseline
        }
        return HostMeasureBatchResponse(responseLongs, responseInts, responseFloats, responseParagraphs, responseLayouts)
    }
}
