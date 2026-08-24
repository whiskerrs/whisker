package rs.whisker.runtime.scene

import android.content.Context
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
}
