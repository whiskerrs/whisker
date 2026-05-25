// Lynx UI subclass that hosts the AndroidX Media3 ExoPlayer +
// PlayerView. A plain `WhiskerUI` subclass — no Whisker
// annotations; registration is driven by `VideoModule`'s
// `definition()` (see `VideoModule.kt`).

package rs.whisker.elements.video

import android.content.Context
import android.view.View
import androidx.media3.common.MediaItem
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

open class VideoView(context: WhiskerContext) : WhiskerUI<View>(context) {

    private var player: ExoPlayer? = null
    private var playerView: PlayerView? = null

    override fun createView(context: Context): View {
        // PlayerView wraps a SurfaceView/TextureView under the
        // hood + draws the playback controls overlay. The ExoPlayer
        // instance is attached via `player =` once a media item
        // is set in `setSrc`.
        val view = PlayerView(context)
        // Hide the built-in controls — Whisker apps drive playback
        // through the DSL's `Function` handlers from Rust.
        view.useController = false
        playerView = view
        return view
    }

    /** Backing of the `src` prop. */
    fun setSrc(value: String) {
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

    fun play() { player?.play() }
    fun pause() { player?.pause() }
    fun seek(seconds: Double) {
        player?.seekTo((seconds * 1000.0).toLong())
    }
}
