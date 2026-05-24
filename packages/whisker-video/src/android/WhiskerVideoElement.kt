// `whisker-video:Video` native element on Android — Phase
// 7-Φ.H.2.6 sample. Backed by `android.widget.VideoView`.
//
// Demonstrates `@WhiskerUIMethod` (KSP forwarder under Phase
// 7-Φ.H.2.2 emits the matching `@LynxUIMethod`-tagged forwarder
// on a `<Class>_LynxBridge` subclass). Imperative dispatch from
// Rust via `ElementRef<VideoProps>::play()` etc. reaches the
// methods below once Phase 7-Φ.H.2.7 wires up the C bridge.

package rs.whisker.elements.video

import android.content.Context
import android.net.Uri
import android.view.View
import android.widget.VideoView
import rs.whisker.annotations.WhiskerElement
import rs.whisker.annotations.WhiskerProp
import rs.whisker.annotations.WhiskerUIMethod
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI
import rs.whisker.runtime.WhiskerValue

@WhiskerElement("Video")
open class WhiskerVideoElement(context: WhiskerContext) : WhiskerUI<View>(context) {

    private var videoView: VideoView? = null

    override fun createView(context: Context): View {
        val vv = VideoView(context)
        videoView = vv
        return vv
    }

    /**
     * `src=` attribute. `@WhiskerProp` routes Lynx's prop dispatch
     * to this setter (the KSP forwarder generates the matching
     * `@LynxProp(name = "src")` wrapper on a bridge subclass).
     * `open` so the bridge subclass can call through. Phase
     * 7-Φ.H.1.
     */
    @WhiskerProp("src")
    open fun setSrc(value: String) {
        videoView?.setVideoURI(Uri.parse(value))
    }

    // ---- @WhiskerUIMethod handlers (Phase 7-Φ.H.2.2) -----------
    //
    // Reachable from Rust via `ElementRef<VideoProps>` once
    // Phase 7-Φ.H.2.7's bridge wiring lands. The KSP forwarder
    // generates a `@LynxUIMethod`-tagged wrapper on
    // `WhiskerVideoElement_LynxBridge` for each — that's what
    // Lynx's `LynxUIMethodsExecutor` reflection finds and
    // dispatches to.

    @WhiskerUIMethod
    open fun play(args: List<WhiskerValue>): WhiskerValue {
        videoView?.start()
        return WhiskerValue.Null
    }

    @WhiskerUIMethod
    open fun pause(args: List<WhiskerValue>): WhiskerValue {
        videoView?.pause()
        return WhiskerValue.Null
    }

    @WhiskerUIMethod
    open fun seek(args: List<WhiskerValue>): WhiskerValue {
        val first = args.firstOrNull()
        val seconds: Double = when (first) {
            is WhiskerValue.Float -> first.value
            is WhiskerValue.Int -> first.value.toDouble()
            else -> return WhiskerValue.Err(
                "seek: expected first arg to be a Float / Int (position in seconds)"
            )
        }
        videoView?.seekTo((seconds * 1000.0).toInt())
        return WhiskerValue.Null
    }
}
