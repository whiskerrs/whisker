package rs.whisker.runtime

import android.content.Context
import android.graphics.Canvas
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Path
import android.text.SpannableString
import android.text.Spanned
import android.text.Layout
import android.text.TextUtils
import android.os.Build
import android.text.style.LeadingMarginSpan
import android.widget.TextView
import kotlin.math.max
import rs.whisker.runtime.internal.CenteredLineHeightSpan

/** Native text element implementing Whisker's single-line decorations. */
public class WhiskerTextView(context: Context) : TextView(context), WhiskerEventSource {
    internal var textEventSink: ((String, WhiskerValue) -> Unit)? = null
    internal var preparedContent: Long = 0
    internal val logicalText: String get() = whiskerTextValue
    private var selectionUpdating = false
    private var nextAccessibilityAction = 0x7f000000
    private val accessibilityActions = mutableMapOf<Int, Long>()
    private var alignedWidth: Int? = null
    private var preparedLayout: Layout? = null
    internal val paragraphLayout: Layout? get() = preparedLayout ?: layout
    override fun installWhiskerEventSink(sink: ((String, WhiskerValue) -> Unit)?) { textEventSink = sink }
    override fun onSelectionChanged(start: Int, end: Int) {
        super.onSelectionChanged(start, end)
        if (!selectionUpdating) textEventSink?.invoke("selectionchange", WhiskerValue.Map(mapOf(
            "revision" to WhiskerValue.Int(preparedContent),
            "start" to WhiskerValue.Int(logicalOffset(minOf(start, end)).toLong()), "end" to WhiskerValue.Int(logicalOffset(maxOf(start, end)).toLong()),
            "direction" to WhiskerValue.Str(if (start <= end) "forward" else "backward"),
        )))
    }

    override fun onTextContextMenuItem(id: Int): Boolean {
        if (id == android.R.id.copy && isTextSelectable) {
            val clipboard = context.getSystemService(android.content.ClipboardManager::class.java)
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("", selectedLogicalText()))
            return true
        }
        return super.onTextContextMenuItem(id)
    }

    override fun onInitializeAccessibilityNodeInfo(info: android.view.accessibility.AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info)
        paragraph?.let { rich ->
            info.text = rich.accessibleText
            rich.accessibleActions.forEach { (span, label) ->
                val id = accessibilityActions.entries.firstOrNull { it.value == span }?.key ?: run {
                    check(nextAccessibilityAction < Int.MAX_VALUE)
                    nextAccessibilityAction++.also { accessibilityActions[it] = span }
                }
                info.addAction(android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction(id, label))
            }
        }
    }

    override fun performAccessibilityAction(action: Int, arguments: android.os.Bundle?): Boolean {
        val span = accessibilityActions[action]
        if (span != null && paragraph?.accessibleActions?.any { it.first == span } == true) {
            textEventSink?.invoke("textactivate", WhiskerValue.Map(mapOf("span" to WhiskerValue.Int(span), "revision" to WhiskerValue.Int(preparedContent))))
            return true
        }
        return super.performAccessibilityAction(action, arguments)
    }

    init {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            setFallbackLineSpacing(true)
        }
    }

    private var whiskerTextValue: String = ""
    internal var paragraph: WhiskerParagraph? = null
    private var whiskerTextIndent: WhiskerTextIndent = WhiskerTextIndent()
    private var whiskerWordBreak: WhiskerTextWordBreak = WhiskerTextWordBreak.NORMAL
    public var whiskerFontFeatures: List<WhiskerFontFeature> = emptyList()
        private set
    public var whiskerFontVariations: List<WhiskerFontVariation> = emptyList()
        private set
    public var whiskerFontOpticalSizing: WhiskerFontOpticalSizing = WhiskerFontOpticalSizing.NONE
        private set
    public var whiskerFontFamilies: List<String> = listOf("system")
        private set
    public var whiskerFontStyle: WhiskerFontStyle = WhiskerFontStyle.NORMAL
        private set
    public var whiskerLineHeight: Float? = null
        private set
    public var whiskerLetterSpacing: Float = 0f
        private set
    public var whiskerDirection: WhiskerTextDirection = WhiskerTextDirection.AUTO
        private set

    public fun setWhiskerText(content: WhiskerTextContent) {
        val sourceChanged = whiskerTextValue != content.value
        alignedWidth = null
        preparedLayout = null
        val preserved = if (whiskerTextValue == content.value && selectionStart >= 0) selectionStart to selectionEnd else null
        if (preparedContent != content.preparedContent) accessibilityActions.clear()
        preparedContent = content.preparedContent
        whiskerTextValue = content.value
        paragraph = content.paragraph
        whiskerTextIndent = content.indent
        whiskerWordBreak = content.wordBreak
        whiskerFontFeatures = content.fontFeatures
        whiskerFontVariations = content.fontVariations
        whiskerFontOpticalSizing = content.fontOpticalSizing
        whiskerFontFamilies = content.fontFamilies
        whiskerFontStyle = content.fontStyle
        whiskerLineHeight = content.lineHeight
        whiskerLetterSpacing = content.letterSpacing
        whiskerDirection = content.direction
        fontFeatureSettings = content.fontFeatures.joinToString(", ") {
            "'${it.tag}' ${it.value}"
        }.ifEmpty { null }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val variations = content.fontVariations.toMutableList()
            if (content.fontOpticalSizing == WhiskerFontOpticalSizing.AUTO &&
                variations.none { it.tag == "opsz" }
            ) {
                variations += WhiskerFontVariation("opsz", content.fontSize)
            }
            fontVariationSettings = variations.joinToString(", ") {
                "'${it.tag}' ${it.value}"
            }.ifEmpty { null }
        }
        setHorizontallyScrolling(!content.wrap)
        maxLines = when {
            !content.wrap -> 1
            content.maxLines > 0 -> content.maxLines
            else -> Int.MAX_VALUE
        }
        ellipsize = if (content.overflow == WhiskerTextOverflow.ELLIPSIS) {
            TextUtils.TruncateAt.END
        } else {
            null
        }
        breakStrategy = if (content.wordBreak == WhiskerTextWordBreak.BREAK_ALL) {
            Layout.BREAK_STRATEGY_SIMPLE
        } else {
            Layout.BREAK_STRATEGY_HIGH_QUALITY
        }
        selectionUpdating = true
        try {
            applyWhiskerText()
            if (preserved != null && text is android.text.Spannable && preserved.second <= text.length) {
                android.text.Selection.setSelection(text as android.text.Spannable, preserved.first, preserved.second)
            }
        } finally { selectionUpdating = false }
        if (sourceChanged && isTextSelectable) onSelectionChanged(selectionStart, selectionEnd)
    }

    public fun installPreparedParagraph(prepared: Layout) {
        val rich = paragraph ?: return
        if (prepared.text.toString() != rich.displayText()) return
        prepared.paint.color = currentTextColor
        val styled = prepared.text as? Spanned
        styled?.getSpans(0, styled.length, ParagraphSpan::class.java)?.forEach { span ->
            span.paintStyle = rich.runs.firstOrNull { it.start == styled.getSpanStart(span) }?.paint
        }
        preparedLayout = prepared
        invalidate()
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        super.onMeasure(widthMeasureSpec, heightMeasureSpec)
        val current = layout ?: return
        if (alignedWidth == current.width) return
        alignedWidth = current.width
        if (paragraph?.resolveVerticalAlignment(current, resources.displayMetrics.density) == true) {
            val start = selectionStart
            val end = selectionEnd
            selectionUpdating = true
            try {
                setText(current.text, BufferType.SPANNABLE)
                if (start >= 0 && end >= 0) android.text.Selection.setSelection(text as android.text.Spannable, start, end)
                super.onMeasure(widthMeasureSpec, heightMeasureSpec)
            } finally { selectionUpdating = false }
        }
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        if (width != oldWidth && (whiskerTextIndent.percentage != 0f || paragraph?.attachments?.any { it.truncation } == true)) applyWhiskerText()
    }

    private fun applyWhiskerText() {
        if (whiskerTextValue.isEmpty()) {
            text = whiskerTextValue
            return
        }
        val density = resources.displayMetrics.density
        val resolvedWidth = if (width > 0) width else layoutParams?.width ?: 0
        val indentPixels = whiskerTextIndent.logicalPixels * density +
            resolvedWidth * whiskerTextIndent.percentage / 100f
        val displayValue = if (paragraph == null && whiskerWordBreak == WhiskerTextWordBreak.KEEP_ALL) {
            protectCjkBreaks(whiskerTextValue)
        } else {
            paragraph?.displayText() ?: whiskerTextValue
        }
        val styled = SpannableString(displayValue).apply {
            paragraph?.apply(this, density, if (resolvedWidth > 0) resolvedWidth / density else Float.POSITIVE_INFINITY)
            setSpan(
                LeadingMarginSpan.Standard(indentPixels.toInt(), 0),
                0,
                length,
                Spanned.SPAN_INCLUSIVE_EXCLUSIVE,
            )
            whiskerLineHeight?.let { lineHeight ->
                setSpan(
                    CenteredLineHeightSpan(lineHeight * density),
                    0,
                    length,
                    Spanned.SPAN_INCLUSIVE_EXCLUSIVE,
                )
            }
        }
        alignedWidth = null
        setText(styled, BufferType.SPANNABLE)
    }

    public var whiskerDecoration: WhiskerTextDecoration? = null
        set(value) {
            field = value
            invalidate()
        }

    override fun onDraw(canvas: Canvas) {
        val rich = paragraph
        val textLayout = paragraphLayout
        if (rich != null && textLayout != null) {
            val save = canvas.save()
            canvas.translate(totalPaddingLeft.toFloat(), extendedPaddingTop.toFloat())
            rich.drawBackgrounds(canvas, textLayout, resources.displayMetrics.density)
            canvas.restoreToCount(save)
        }
        if (preparedLayout != null && !isTextSelectable) {
            val save = canvas.save()
            canvas.translate(totalPaddingLeft.toFloat(), extendedPaddingTop.toFloat())
            preparedLayout?.draw(canvas)
            canvas.restoreToCount(save)
        } else super.onDraw(canvas)
        if (rich != null && textLayout != null) {
            val save = canvas.save()
            canvas.translate(totalPaddingLeft.toFloat(), extendedPaddingTop.toFloat())
            rich.drawDecorations(canvas, textLayout, paint, resources.displayMetrics.density)
            canvas.restoreToCount(save)
            if (rich.runs.isNotEmpty()) return
        }
        val decoration = whiskerDecoration ?: return
        if (textLayout == null) return
        val density = resources.displayMetrics.density
        val stroke = max(density, textSize / 16f)
        val decorationPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = decoration.color
            style = Paint.Style.STROKE
            strokeWidth = stroke
        }
        for (line in 0 until textLayout.lineCount) {
            val left = totalPaddingLeft + textLayout.getLineLeft(line)
            val right = totalPaddingLeft + textLayout.getLineRight(line)
            if (right <= left) continue
            val baseline = extendedPaddingTop + textLayout.getLineBaseline(line)
            val y = when (decoration.line) {
                WhiskerTextDecorationLine.UNDERLINE -> baseline + stroke * 1.5f
                WhiskerTextDecorationLine.LINE_THROUGH -> baseline + paint.fontMetrics.ascent * 0.35f
            }
            drawWhiskerTextDecoration(canvas, decorationPaint, decoration.style, left, right, y, stroke)
        }
    }


}

private fun protectCjkBreaks(value: String): String = buildString {
    var previousWasCjk = false
    value.forEach { character ->
        val currentIsCjk = character.isCjk()
        if (previousWasCjk && currentIsCjk) append('\u2060')
        append(character)
        previousWasCjk = currentIsCjk
    }
}

private fun Char.isCjk(): Boolean = code in 0x2E80..0x9FFF || code in 0xF900..0xFAFF ||
    code in 0xAC00..0xD7AF
