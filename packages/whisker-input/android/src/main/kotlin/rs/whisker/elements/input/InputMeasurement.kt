package rs.whisker.elements.input

import android.graphics.Typeface
import android.os.Build
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import android.widget.EditText
import org.json.JSONObject
import rs.whisker.runtime.WhiskerAvailableSpace
import rs.whisker.runtime.WhiskerMeasureRequest
import rs.whisker.runtime.WhiskerMeasuredSize
import rs.whisker.runtime.WhiskerTextStyle
import rs.whisker.runtime.WhiskerFontStyle
import kotlin.math.ceil
import kotlin.math.max

internal object InputTypography {
    fun typeface(family: String?, weight: Int, italic: Boolean): Typeface {
        val base = Typeface.create(family, Typeface.NORMAL)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            Typeface.create(base, weight.coerceIn(1, 1000), italic)
        } else {
            Typeface.create(base, (if (weight >= 600) Typeface.BOLD else 0) or (if (italic) Typeface.ITALIC else 0))
        }
    }

    fun spacing(paint: TextPaint, lineHeight: Float?): Float =
        lineHeight?.let { it - (paint.fontMetricsInt.descent - paint.fontMetricsInt.ascent) } ?: 0f

    fun apply(view: EditText, style: WhiskerTextStyle) {
        val density = view.resources.displayMetrics.density
        view.setTextSize(android.util.TypedValue.COMPLEX_UNIT_PX, inputTextSizePixels(style.fontSize, density))
        view.typeface = typeface(style.fontFamilies.firstOrNull()?.takeUnless { it == "system" }, style.fontWeight, style.fontStyle != WhiskerFontStyle.NORMAL)
        view.letterSpacing = style.letterSpacing / style.fontSize
        view.includeFontPadding = false
        view.breakStrategy = Layout.BREAK_STRATEGY_SIMPLE
        view.hyphenationFrequency = Layout.HYPHENATION_FREQUENCY_NONE
        view.setLineSpacing(spacing(view.paint, style.lineHeight?.times(density)), 1f)
    }
}

internal fun measureInput(request: WhiskerMeasureRequest): WhiskerMeasuredSize? {
    if (request.payloadVersion != 1) return null
    val bytes = request.payload.asBytes() ?: return null
    val data = try { JSONObject(bytes.toString(Charsets.UTF_8)) } catch (_: org.json.JSONException) { return null }
    val scale = data.getDouble("scale_factor").toFloat()
    val paint = TextPaint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
        textSize = data.getDouble("font_size").toFloat() * scale
        typeface = InputTypography.typeface(if (data.isNull("font_family")) null else data.getString("font_family"), data.getInt("font_weight"), data.getBoolean("italic"))
        letterSpacing = data.getDouble("letter_spacing").toFloat() * scale / textSize
    }
    val text = data.getString("text")
    val multiline = data.getBoolean("multiline")
    val content = if (multiline) text else text.replace('\n', ' ')
    val desired = Layout.getDesiredWidth(content, paint)
    val width = request.knownWidth?.times(scale) ?: when (request.availableWidthKind) {
        WhiskerAvailableSpace.DEFINITE -> minOf(desired, (request.availableWidth ?: 0f) * scale)
        WhiskerAvailableSpace.MIN_CONTENT -> if (multiline) 1f else desired
        WhiskerAvailableSpace.MAX_CONTENT -> desired
    }
    val lineHeight = if (data.isNull("line_height")) null else data.getDouble("line_height").toFloat() * scale
    val layout = StaticLayout.Builder.obtain(content, 0, content.length, paint, ceil(width.toDouble()).toInt().coerceAtLeast(1))
        .setIncludePad(false)
        .setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE)
        .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE)
        .setMaxLines(if (multiline) Int.MAX_VALUE else 1)
        .setLineSpacing(InputTypography.spacing(paint, lineHeight), 1f)
        .build()
    var usedWidth = 0f
    for (line in 0 until layout.lineCount) usedWidth = max(usedWidth, layout.getLineWidth(line))
    return WhiskerMeasuredSize(request.knownWidth ?: usedWidth / scale, request.knownHeight ?: layout.height / scale)
}
