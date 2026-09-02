// Whisker module view that hosts the AndroidX Media3 ExoPlayer +
// PlayerView. Registration is driven by `VideoModule`'s `definition()`,
// not by annotations here.

package rs.whisker.elements.video

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.view.View
import androidx.media3.common.MediaItem
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerUI

open class VideoView(context: WhiskerContext) : WhiskerUI<View>(context) {

    private var player: ExoPlayer? = null
    private var playerView: PlayerView? = null
    private val cleanupHandler = Handler(Looper.getMainLooper())

    override fun createView(context: Context): View {
        val view = PlayerView(context)
        // Whisker apps drive playback through the DSL's `Function`
        // handlers from Rust, so PlayerView's own controls stay off.
        view.useController = false
        playerView = view
        return view
    }

    /** Backing of the `src` prop. */
    fun setSrc(value: String) {
        // Media3 requires `release()` before the last reference is
        // dropped, or the audio session leaks.
        player?.release()
        player = null
        playerView?.player = null
        if (value.isEmpty()) return

        val ctx = view().context
        val p = ExoPlayer.Builder(ctx).build()
        playerView?.player = p
        p.setMediaItem(MediaItem.fromUri(value))
        p.prepare()
        // TODO: expose this as an `autoplay` attribute instead of
        // unconditionally starting playback.
        p.playWhenReady = true
        player = p
    }

    fun play() { player?.play() }
    fun pause() { player?.pause() }
    fun seek(seconds: Double) {
        player?.seekTo((seconds.coerceAtLeast(0.0) * 1000.0).toLong())
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        // Reparenting can detach and attach synchronously in one Host batch.
        // Defer cleanup one main-loop turn so that path keeps its player.
        cleanupHandler.post {
            if (!isAttachedToWindow) setSrc("")
        }
    }
}
