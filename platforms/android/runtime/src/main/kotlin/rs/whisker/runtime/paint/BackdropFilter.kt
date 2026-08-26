package rs.whisker.runtime.paint

import android.graphics.Canvas
import android.graphics.Bitmap
import android.graphics.Path
import android.graphics.RenderEffect
import android.graphics.RenderNode
import android.graphics.Shader
import android.view.View
import androidx.annotation.RequiresApi
import rs.whisker.runtime.WhiskerView
import rs.whisker.runtime.scene.HostNode

/** API 31+ renderer for Whisker's single backdrop blur primitive. */
@RequiresApi(31)
internal class HostBackdropBlurRenderer {
    private val backdrop = RenderNode("whisker-backdrop")

    fun draw(
        canvas: Canvas,
        root: WhiskerView,
        target: HostNode,
        radiusPx: Float,
        clip: Path,
    ) {
        if (radiusPx <= 0f || root.width <= 0 || root.height <= 0) return
        if (!canvas.isHardwareAccelerated) {
            drawSoftware(canvas, root, target, radiusPx, clip)
            return
        }

        backdrop.setPosition(0, 0, root.width, root.height)
        val recordingCanvas = backdrop.beginRecording(root.width, root.height)
        try {
            root.recordBackdrop(recordingCanvas, target)
        } finally {
            backdrop.endRecording()
        }
        backdrop.setRenderEffect(
            RenderEffect.createBlurEffect(radiusPx, radiusPx, Shader.TileMode.CLAMP),
        )

        val offset = descendantOffset(root, target)
        val save = canvas.save()
        canvas.clipPath(clip)
        canvas.translate(-offset[0], -offset[1])
        canvas.drawRenderNode(backdrop)
        canvas.restoreToCount(save)
    }

    private fun drawSoftware(
        canvas: Canvas,
        root: WhiskerView,
        target: HostNode,
        radiusPx: Float,
        clip: Path,
    ) {
        val bitmap = Bitmap.createBitmap(root.width, root.height, Bitmap.Config.ARGB_8888)
        root.recordBackdrop(Canvas(bitmap), target)
        boxBlur(bitmap, radiusPx.toInt().coerceIn(1, 64))

        val offset = descendantOffset(root, target)
        val save = canvas.save()
        canvas.clipPath(clip)
        canvas.translate(-offset[0], -offset[1])
        canvas.drawBitmap(bitmap, 0f, 0f, null)
        canvas.restoreToCount(save)
        bitmap.recycle()
    }

    /** Two linear passes keep the software/screenshot fallback bounded. */
    private fun boxBlur(bitmap: Bitmap, radius: Int) {
        val width = bitmap.width
        val height = bitmap.height
        val source = IntArray(width * height)
        val temporary = IntArray(source.size)
        val output = IntArray(source.size)
        bitmap.getPixels(source, 0, width, 0, 0, width, height)
        val diameter = radius * 2 + 1

        for (y in 0 until height) {
            var alpha = 0
            var red = 0
            var green = 0
            var blue = 0
            for (offset in -radius..radius) {
                val color = source[y * width + offset.coerceIn(0, width - 1)]
                alpha += color ushr 24
                red += color ushr 16 and 0xff
                green += color ushr 8 and 0xff
                blue += color and 0xff
            }
            for (x in 0 until width) {
                temporary[y * width + x] =
                    (alpha / diameter shl 24) or (red / diameter shl 16) or
                    (green / diameter shl 8) or (blue / diameter)
                val leaving = source[y * width + (x - radius).coerceIn(0, width - 1)]
                val entering = source[y * width + (x + radius + 1).coerceIn(0, width - 1)]
                alpha += (entering ushr 24) - (leaving ushr 24)
                red += (entering ushr 16 and 0xff) - (leaving ushr 16 and 0xff)
                green += (entering ushr 8 and 0xff) - (leaving ushr 8 and 0xff)
                blue += (entering and 0xff) - (leaving and 0xff)
            }
        }
        for (x in 0 until width) {
            var alpha = 0
            var red = 0
            var green = 0
            var blue = 0
            for (offset in -radius..radius) {
                val color = temporary[offset.coerceIn(0, height - 1) * width + x]
                alpha += color ushr 24
                red += color ushr 16 and 0xff
                green += color ushr 8 and 0xff
                blue += color and 0xff
            }
            for (y in 0 until height) {
                output[y * width + x] =
                    (alpha / diameter shl 24) or (red / diameter shl 16) or
                    (green / diameter shl 8) or (blue / diameter)
                val leaving = temporary[(y - radius).coerceIn(0, height - 1) * width + x]
                val entering = temporary[(y + radius + 1).coerceIn(0, height - 1) * width + x]
                alpha += (entering ushr 24) - (leaving ushr 24)
                red += (entering ushr 16 and 0xff) - (leaving ushr 16 and 0xff)
                green += (entering ushr 8 and 0xff) - (leaving ushr 8 and 0xff)
                blue += (entering and 0xff) - (leaving and 0xff)
            }
        }
        bitmap.setPixels(output, 0, width, 0, 0, width, height)
    }

    private fun descendantOffset(root: View, target: View): FloatArray {
        var x = 0f
        var y = 0f
        var current: View? = target
        while (current != null && current !== root) {
            x += current.x - current.scrollX
            y += current.y - current.scrollY
            current = current.parent as? View
        }
        check(current === root) { "backdrop target must remain below its Whisker root" }
        return floatArrayOf(x, y)
    }
}
