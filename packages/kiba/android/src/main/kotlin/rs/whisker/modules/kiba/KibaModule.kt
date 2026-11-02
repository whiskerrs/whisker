package rs.whisker.modules.kiba

import android.content.Context
import android.view.Choreographer
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerUI

class KibaSurfaceView(context: Context) :
    SurfaceView(context), SurfaceHolder.Callback, Choreographer.FrameCallback {
    private var handle = 0L
    private var width = 0
    private var height = 0

    init {
        holder.addCallback(this)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        handle = nativeCreate(holder.surface)
        Choreographer.getInstance().postFrameCallback(this)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        this.width = width
        this.height = height
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        Choreographer.getInstance().removeFrameCallback(this)
        if (handle != 0L) {
            nativeDestroy(handle)
            handle = 0L
        }
    }

    override fun doFrame(frameTimeNanos: Long) {
        if (handle == 0L) return
        nativeRender(handle, width, height)
        Choreographer.getInstance().postFrameCallback(this)
    }

    private external fun nativeCreate(surface: Surface): Long
    private external fun nativeRender(handle: Long, width: Int, height: Int)
    private external fun nativeDestroy(handle: Long)
}

class KibaViewport(context: WhiskerContext) : WhiskerUI<KibaSurfaceView>(context) {
    override fun createView(context: Context): KibaSurfaceView = KibaSurfaceView(context)
}

@WhiskerModule
class KibaModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("Kiba")
        View("kiba:Viewport", KibaViewport::class.java) {}
    }
}
