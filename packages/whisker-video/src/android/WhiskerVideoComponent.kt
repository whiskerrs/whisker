// `whisker-video:Video` platform component on Android — Phase
// 7-Φ.H.2.6 sample. Backed by AndroidX Media3 ExoPlayer +
// `PlayerView` (the modern replacement for the deprecated
// `MediaPlayer` / `VideoView` pair).
//
// Demonstrates:
//   - `@WhiskerComponent` for tag registration (Phase 7-Φ.H.2.1
//     namespaced as `whisker-video:Video`).
//   - `@WhiskerProp("src")` for declarative prop dispatch from
//     Lynx's reflection layer.
//   - `@WhiskerUIMethod` for imperative methods Rust dispatches
//     via `ElementRef<T>` (`video_ref.play()` etc.).

package rs.whisker.elements.video

import android.content.Context
import android.view.View
import androidx.media3.common.MediaItem
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import rs.whisker.annotations.WhiskerComponent
import rs.whisker.annotations.WhiskerProp
import rs.whisker.annotations.WhiskerUIMethod
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI
import rs.whisker.runtime.WhiskerValue

@WhiskerComponent("Video")
open class WhiskerVideoComponent(context: WhiskerContext) : WhiskerUI<View>(context) {

    private var player: ExoPlayer? = null
    private var playerView: PlayerView? = null

    override fun createView(context: Context): View {
        // PlayerView wraps a SurfaceView/TextureView under the
        // hood + draws the playback controls overlay. The ExoPlayer
        // instance is attached via `player =` once a media item
        // is set in `setSrc`.
        val view = PlayerView(context)
        // Hide the built-in controls — Whisker apps drive playback
        // through the @WhiskerUIMethod handlers below from Rust.
        view.useController = false
        playerView = view
        return view
    }

    @WhiskerProp("src")
    open fun setSrc(value: String) {
        // Tear down any prior player so a `src=` change rebuilds
        // cleanly. Media3 requires `release()` before dropping the
        // last reference; without it the audio session leaks.
        player?.release()

        val ctx = view?.context ?: return
        val p = ExoPlayer.Builder(ctx).build()
        playerView?.player = p
        p.setMediaItem(MediaItem.fromUri(value))
        p.prepare()
        // Autoplay so the demo shows motion immediately. Real
        // modules would expose this via an `autoplay` attribute.
        p.playWhenReady = true
        player = p
    }

    // ---- @WhiskerUIMethod handlers ---------------------------
    //
    // Reachable from Rust via `ElementRef<VideoProps>`. The KSP
    // forwarder generates a `@LynxUIMethod`-tagged wrapper on
    // `WhiskerVideoComponent_LynxBridge` for each — Lynx's
    // `LynxUIMethodsExecutor` reflection finds and dispatches to
    // those.

    @WhiskerUIMethod
    open fun play(args: List<WhiskerValue>): WhiskerValue {
        player?.play()
        return WhiskerValue.Null
    }

    @WhiskerUIMethod
    open fun pause(args: List<WhiskerValue>): WhiskerValue {
        player?.pause()
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
        player?.seekTo((seconds * 1000.0).toLong())
        return WhiskerValue.Null
    }
}
