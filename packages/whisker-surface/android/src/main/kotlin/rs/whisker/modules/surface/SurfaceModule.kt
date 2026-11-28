package rs.whisker.modules.surface

import android.content.Context
import android.graphics.SurfaceTexture
import android.view.Surface
import android.view.TextureView
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerUI

/** Calls Rust only from [WhiskerSurface.onFrame], never from a prop setter that runs inside Whisker's commit. */
class WhiskerSurfaceView(context: Context) : TextureView(context), TextureView.SurfaceTextureListener {
    private var surfaceId = 0L
    private var attachedId = 0L
    private var surface: Surface? = null
    private var reportedWidth = 0
    private var reportedHeight = 0
    internal var requestFrame: () -> Unit = {}

    init {
        surfaceTextureListener = this
    }

    fun setSurface(value: String) {
        val id = value.substringBefore(':').toLongOrNull() ?: 0L
        if (id != surfaceId) {
            detach()
            surfaceId = id
        }
        requestFrame()
    }

    override fun onSurfaceTextureAvailable(texture: SurfaceTexture, width: Int, height: Int) {
        surface = Surface(texture)
        requestFrame()
    }

    override fun onSurfaceTextureSizeChanged(texture: SurfaceTexture, width: Int, height: Int) {
        requestFrame()
    }

    override fun onSurfaceTextureDestroyed(texture: SurfaceTexture): Boolean {
        detach()
        surface?.release()
        surface = null
        return true
    }

    override fun onSurfaceTextureUpdated(texture: SurfaceTexture) = Unit

    override fun onWindowVisibilityChanged(visibility: Int) {
        super.onWindowVisibilityChanged(visibility)
        if (visibility == VISIBLE) requestFrame()
    }

    /** Returns whether the surface wants another frame. */
    internal fun frame(frameTimeNanos: Long): Boolean {
        if (windowVisibility != VISIBLE) return false
        val target = surface ?: return false
        val id = surfaceId
        if (id == 0L || width == 0 || height == 0) return false
        val scale = resources.displayMetrics.density
        if (attachedId != id) {
            nativeAttach(id, target, width, height, scale)
            attachedId = id
            reportedWidth = width
            reportedHeight = height
        } else if (reportedWidth != width || reportedHeight != height) {
            nativeResize(id, width, height, scale)
            reportedWidth = width
            reportedHeight = height
        }
        return nativeFrame(id, frameTimeNanos)
    }

    private fun detach() {
        if (attachedId != 0L) {
            nativeDetach(attachedId)
            attachedId = 0L
        }
    }

    private external fun nativeAttach(id: Long, surface: Surface, width: Int, height: Int, scale: Float)
    private external fun nativeResize(id: Long, width: Int, height: Int, scale: Float)
    private external fun nativeFrame(id: Long, frameTimeNanos: Long): Boolean
    private external fun nativeDetach(id: Long)
}

class WhiskerSurface(context: WhiskerContext) : WhiskerUI<WhiskerSurfaceView>(context) {
    init {
        view().requestFrame = ::requestFrame
    }

    override fun createView(context: Context): WhiskerSurfaceView = WhiskerSurfaceView(context)

    override fun onFrame(frameTimeNanos: Long) {
        if (view().frame(frameTimeNanos)) requestFrame()
    }
}

@WhiskerModule
class SurfaceModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Surface")
        View("whisker-surface:Surface", WhiskerSurface::class.java) {
            Prop("surface") { view: WhiskerSurface, value ->
                view.view().setSurface(value.asString() ?: "")
            }
        }
    }
}
