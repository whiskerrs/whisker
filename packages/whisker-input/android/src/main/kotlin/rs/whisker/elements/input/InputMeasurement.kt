package rs.whisker.elements.input

import android.graphics.Typeface
import android.os.Build
import android.text.Layout
import android.text.SpannableString
import android.text.StaticLayout
import android.text.TextPaint
import android.widget.EditText
import rs.whisker.runtime.WhiskerValue
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

    fun apply(view: EditText, style: WhiskerTextStyle) {
        val density = view.resources.displayMetrics.density
        view.setTextSize(android.util.TypedValue.COMPLEX_UNIT_PX, inputTextSizePixels(style.fontSize, density))
        view.typeface = typeface(style.fontFamilies.firstOrNull()?.takeUnless { it == "system" }, style.fontWeight, style.fontStyle != WhiskerFontStyle.NORMAL)
        view.letterSpacing = style.letterSpacing / style.fontSize
        view.includeFontPadding = false
        view.breakStrategy = Layout.BREAK_STRATEGY_SIMPLE
        view.hyphenationFrequency = Layout.HYPHENATION_FREQUENCY_NONE
        // CSS line-height is applied by InputLineHeightSpan, not line spacing.
        view.setLineSpacing(0f, 1f)
        // Pin the API 28+ default the measurer mirrors in configureFallback.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) view.isFallbackLineSpacing = true
        // API 35 raises each line to the locale's preferred minimum height
        // (a CJK font's for `ja`). The measurer has no such minimum, and the
        // span-less empty line after a trailing newline would outgrow the
        // measured height by the difference.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.VANILLA_ICE_CREAM) {
            view.setLocalePreferredLineHeightForMinimumUsed(false)
        }
    }

    /** Mirrors the EditText's fallback line spacing in a measuring layout. */
    fun configureFallback(builder: StaticLayout.Builder): StaticLayout.Builder {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) builder.setUseLineSpacingFromFallbacks(true)
        return builder
    }
}

internal fun measureInput(request: WhiskerMeasureRequest): WhiskerMeasuredSize? {
    if (request.payloadVersion != 1) return null
    val data = (request.payload as? WhiskerValue.Map)?.value ?: return null
    val scale = data["scale_factor"]?.asDouble()?.toFloat() ?: return null
    val fontSize = data["font_size"]?.asDouble()?.toFloat() ?: return null
    val weight = data["font_weight"]?.asInt()?.toInt() ?: return null
    val italic = data["italic"]?.asBool() ?: return null
    val spacing = data["letter_spacing"]?.asDouble()?.toFloat() ?: return null
    val text = data["text"]?.asString() ?: return null
    val multiline = data["multiline"]?.asBool() ?: return null
    val paint = TextPaint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply {
        textSize = fontSize * scale
        typeface = InputTypography.typeface(data["font_family"]?.asString(), weight, italic)
        letterSpacing = spacing * scale / textSize
    }
    val content = if (multiline) text else text.replace('\n', ' ')
    val desired = Layout.getDesiredWidth(content, paint)
    val width = request.knownWidth?.times(scale) ?: when (request.availableWidthKind) {
        WhiskerAvailableSpace.DEFINITE -> minOf(desired, (request.availableWidth ?: 0f) * scale)
        WhiskerAvailableSpace.MIN_CONTENT -> if (multiline) 1f else desired
        WhiskerAvailableSpace.MAX_CONTENT -> desired
    }
    val box = InputLineBox.of(paint, data["line_height"]?.asDouble()?.toFloat()?.times(scale))
    val styled: CharSequence = if (box == null) content else SpannableString(content).also { applyInputLineBox(it, box) }
    val layout = InputTypography.configureFallback(StaticLayout.Builder.obtain(styled, 0, styled.length, paint, ceil(width.toDouble()).toInt().coerceAtLeast(1)))
        .setIncludePad(false)
        .setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE)
        .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE)
        .setMaxLines(if (multiline) Int.MAX_VALUE else 1)
        .setLineSpacing(0f, 1f)
        .build()
    var usedWidth = 0f
    for (line in 0 until layout.lineCount) usedWidth = max(usedWidth, layout.getLineWidth(line))
    // The view carries the line box's half-leading as padding (InputLineBox).
    val height = layout.height + (box?.paddingTop ?: 0) + (box?.paddingBottom ?: 0)
    return WhiskerMeasuredSize(request.knownWidth ?: usedWidth / scale, request.knownHeight ?: height / scale)
}
