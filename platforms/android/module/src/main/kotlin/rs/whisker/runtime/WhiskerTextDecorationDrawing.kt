package rs.whisker.runtime

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.DashPathEffect

internal fun drawWhiskerTextDecoration(
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
