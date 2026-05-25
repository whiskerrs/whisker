// Phase L-3 — `whisker-video` migrated to the new
// `ModuleDefinition` DSL on Android.
//
// Replaces the pre-L-3 `@WhiskerComponent("Video")`-annotated
// `WhiskerVideoComponent` class plus its `@WhiskerProp` /
// `@WhiskerUIMethod` annotated members. The Lynx tag stays
// `whisker-video:Video`; the DSL's `Prop("src") { ... }`,
// `Function("play") { ... }`, etc. expand into the same Lynx-
// visible setters / invokers via the KSP L-2c + runtime install.
//
// Same shape on iOS — `VideoModule.swift` next to this file.

package rs.whisker.elements.video

import android.content.Context
import android.view.View
import androidx.media3.common.MediaItem
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.ui.PlayerView
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerUI

/**
 * Lynx UI subclass that hosts the AndroidX Media3 ExoPlayer +
 * PlayerView. Stays a plain [WhiskerUI] subclass — no Whisker
 * annotations; registration is driven by [VideoModule] below.
 */
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
        // through the DSL's `Function` handlers below from Rust.
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

/**
 * DSL-driven module. Subclasses [WhiskerModule] and declares:
 *   - Tag name `Video` (registers as `whisker-video:Video`).
 *   - View class [VideoView].
 *   - One prop setter (`src`).
 *   - Three sync method dispatchers (`play`, `pause`, `seek`).
 *
 * The KSP L-2c processor finds this class by superclass-chain
 * walk and emits the registration block into
 * `WhiskerVideoBehaviors.registerAll()`.
 */
class VideoModule : WhiskerModule() {
    override fun definition() = ModuleDefinition {
        Name("Video")
        View(VideoView::class.java) {
            Prop("src") { view: VideoView, value: String ->
                view.setSrc(value)
            }
            Function("play") { view: VideoView -> view.play() }
            Function("pause") { view: VideoView -> view.pause() }
            Function("seek") { view: VideoView, seconds: Double ->
                view.seek(seconds)
            }
        }
    }
}
