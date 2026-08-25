package rs.whisker.runtime.scene

import android.content.Context
import android.graphics.Canvas
import android.graphics.Matrix
import rs.whisker.runtime.WhiskerContainerView
import rs.whisker.runtime.WhiskerMountedElement
import rs.whisker.runtime.paint.HostBoxPaint

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
    var mountedElement: WhiskerMountedElement? = null
    var zOrder: Int = 0

    private val localTransform = Matrix()
    private var hasLocalTransform = false

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
            super.draw(canvas)
            return
        }
        val save = canvas.save()
        canvas.concat(localTransform)
        super.draw(canvas)
        canvas.restoreToCount(save)
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
