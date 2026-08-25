package rs.whisker.runtime

import android.content.Context
import android.graphics.Canvas
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Path
import android.text.SpannableString
import android.text.Spanned
import android.text.style.LeadingMarginSpan
import android.widget.TextView
import kotlin.math.max

/** Native text element with Lynx-compatible single-line decorations. */
public class WhiskerTextView(context: Context) : TextView(context) {
    private var whiskerTextValue: String = ""
    private var whiskerTextIndent: WhiskerTextIndent = WhiskerTextIndent()

    public fun setWhiskerText(value: String, indent: WhiskerTextIndent) {
        whiskerTextValue = value
        whiskerTextIndent = indent
        applyWhiskerText()
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        if (width != oldWidth && whiskerTextIndent.percentage != 0f) applyWhiskerText()
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
        text = SpannableString(whiskerTextValue).apply {
            setSpan(
                LeadingMarginSpan.Standard(indentPixels.toInt(), 0),
                0,
                length,
                Spanned.SPAN_INCLUSIVE_EXCLUSIVE,
            )
        }
    }

    public var whiskerDecoration: WhiskerTextDecoration? = null
        set(value) {
            field = value
            invalidate()
        }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val decoration = whiskerDecoration ?: return
        val textLayout = layout ?: return
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
            drawDecoration(canvas, decorationPaint, decoration.style, left, right, y, stroke)
        }
    }

    private fun drawDecoration(
        canvas: Canvas,
        paint: Paint,
        style: WhiskerTextDecorationStyle,
        left: Float,
        right: Float,
        y: Float,
        stroke: Float,
    ) {
        paint.pathEffect = null
        paint.strokeCap = Paint.Cap.BUTT
        when (style) {
            WhiskerTextDecorationStyle.SOLID -> canvas.drawLine(left, y, right, y, paint)
            WhiskerTextDecorationStyle.DOUBLE -> {
                canvas.drawLine(left, y - stroke, right, y - stroke, paint)
                canvas.drawLine(left, y + stroke, right, y + stroke, paint)
            }
            WhiskerTextDecorationStyle.DOTTED -> {
                paint.strokeCap = Paint.Cap.ROUND
                paint.pathEffect = DashPathEffect(floatArrayOf(stroke, stroke * 2f), 0f)
                canvas.drawLine(left, y, right, y, paint)
            }
            WhiskerTextDecorationStyle.DASHED -> {
                paint.pathEffect = DashPathEffect(floatArrayOf(stroke * 4f, stroke * 2f), 0f)
                canvas.drawLine(left, y, right, y, paint)
            }
            WhiskerTextDecorationStyle.WAVY -> {
                val path = Path().apply { moveTo(left, y) }
                val step = stroke * 2f
                var x = left
                var up = true
                while (x < right) {
                    x = (x + step).coerceAtMost(right)
                    path.lineTo(x, y + if (up) -stroke else stroke)
                    up = !up
                }
                canvas.drawPath(path, paint)
            }
        }
    }
}
