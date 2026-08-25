package rs.whisker.runtime.scene

import android.content.Context
import android.graphics.Canvas
import android.graphics.Matrix
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RectF
import rs.whisker.runtime.WhiskerContainerView
import rs.whisker.runtime.WhiskerMountedElement
import rs.whisker.runtime.paint.HostBoxPaint
import rs.whisker.runtime.paint.HostBackgroundLayers
import rs.whisker.runtime.paint.HostBoxShadow
import rs.whisker.runtime.paint.HostClipPath
import rs.whisker.runtime.paint.ResolvedBoxGeometry
import rs.whisker.runtime.paint.normalizeRadii
import rs.whisker.runtime.paint.drawInsetBoxShadows
import rs.whisker.runtime.paint.drawOuterBoxShadows

/** Mutable logical geometry attached to one Host scene node. */
internal data class HostGeometry(
    var x: Float = 0f,
    var y: Float = 0f,
    var width: Float = 0f,
    var height: Float = 0f,
    var contentX: Float = 0f,
    var contentY: Float = 0f,
    var contentWidth: Float = 0f,
    var contentHeight: Float = 0f,
)

/**
 * Common Android wrapper for every built-in or custom Whisker element.
 *
 * The scene owner controls hierarchy and geometry. Element modules only own
 * the mounted content View placed inside this wrapper.
 */
internal class HostNode(context: Context, val element: String) : WhiskerContainerView(context) {
    val geometry = HostGeometry()
    var paint: HostBoxPaint? = null
    var backgroundLayers: HostBackgroundLayers? = null
    var boxShadows: List<HostBoxShadow> = emptyList()
    var clipPath: HostClipPath? = null
    var mountedElement: WhiskerMountedElement? = null
    var zOrder: Int = 0

    private val localTransform = Matrix()
    private var hasLocalTransform = false
    private var overflowClipRect = RectF()
    private var overflowClipPath: Path? = null
    private var paintClipPath: Path? = null
    private var resolvedBoxGeometry: ResolvedBoxGeometry? = null

    init {
        setWillNotDraw(false)
    }

    fun setOverflowClipGeometry(geometry: ResolvedBoxGeometry) {
        resolvedBoxGeometry = geometry
        val top = geometry.borderWidths[0].coerceIn(0f, geometry.height)
        val right = geometry.borderWidths[1].coerceIn(0f, geometry.width)
        val bottom = geometry.borderWidths[2].coerceIn(0f, geometry.height)
        val left = geometry.borderWidths[3].coerceIn(0f, geometry.width)
        overflowClipRect = RectF(left, top, geometry.width - right, geometry.height - bottom)
        if (overflowClipRect.isEmpty) {
            overflowClipPath = Path()
            invalidate()
            return
        }
        val outer = geometry.cornerRadii
        val inner = normalizeRadii(
            floatArrayOf(
                (outer[0] - left).coerceAtLeast(0f),
                (outer[1] - top).coerceAtLeast(0f),
                (outer[2] - right).coerceAtLeast(0f),
                (outer[3] - top).coerceAtLeast(0f),
                (outer[4] - right).coerceAtLeast(0f),
                (outer[5] - bottom).coerceAtLeast(0f),
                (outer[6] - left).coerceAtLeast(0f),
                (outer[7] - bottom).coerceAtLeast(0f),
            ),
            overflowClipRect.width(),
            overflowClipRect.height(),
        )
        overflowClipPath = Path().apply {
            addRoundRect(overflowClipRect, inner, Path.Direction.CW)
        }
        invalidate()
    }

    fun setPaintClipPath(path: Path?) {
        paintClipPath = path
        invalidate()
        (parent as? android.view.View)?.invalidate()
    }

    fun resolvedBorderWidths(): FloatArray = resolvedBoxGeometry?.borderWidths ?: FloatArray(4)

    /** Applies a protocol transform around the local border-box origin. */
    fun setLocalTransform(values: FloatArray, density: Float) {
        require(isSupported2dTransform(values))
        (parent as? android.view.View)?.invalidate()
        localTransform.setValues(
            floatArrayOf(
                values[0], values[4], values[12] * density,
                values[1], values[5], values[13] * density,
                values[3], values[7], values[15],
            ),
        )
        hasLocalTransform = !localTransform.isIdentity
        invalidate()
        (parent as? android.view.View)?.invalidate()
    }

    override fun draw(canvas: Canvas) {
        if (!hasLocalTransform) {
            drawClipped(canvas)
            return
        }
        val save = canvas.save()
        canvas.concat(localTransform)
        drawClipped(canvas)
        canvas.restoreToCount(save)
    }

    private fun drawClipped(canvas: Canvas) {
        val save = canvas.save()
        paintClipPath?.let(canvas::clipPath)
        drawOuterBoxShadows(canvas, resolvedBoxGeometry, boxShadows)
        super.draw(canvas)
        canvas.restoreToCount(save)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        drawInsetBoxShadows(canvas, resolvedBoxGeometry, boxShadows)
    }

    override fun clipDescendants(
        canvas: Canvas,
        horizontal: Boolean,
        vertical: Boolean,
        visible: Rect,
    ) {
        val path = overflowClipPath
        if (path == null) {
            super.clipDescendants(canvas, horizontal, vertical, visible)
            return
        }
        if (horizontal && vertical) {
            canvas.clipPath(path)
            return
        }
        canvas.clipRect(
            if (horizontal) overflowClipRect.left else visible.left.toFloat(),
            if (vertical) overflowClipRect.top else visible.top.toFloat(),
            if (horizontal) overflowClipRect.right else visible.right.toFloat(),
            if (vertical) overflowClipRect.bottom else visible.bottom.toFloat(),
        )
    }
}

/** True only for finite 2D affine matrices embedded in a column-major 4x4 matrix. */
internal fun isSupported2dTransform(values: FloatArray): Boolean =
    values.size == 16 && values.all(Float::isFinite) &&
        values[2] == 0f && values[3] == 0f &&
        values[6] == 0f && values[7] == 0f &&
        values[8] == 0f && values[9] == 0f &&
        values[10] == 1f && values[11] == 0f &&
        values[14] == 0f && values[15] == 1f
