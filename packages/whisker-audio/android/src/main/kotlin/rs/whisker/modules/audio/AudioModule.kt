// `whisker-audio` Module (Android). View-less.
//
// Each Rust-side `Player` allocation calls `create(id, source)`, drives
// its entry through `play` / `pause` / `seekTo`, and `release(id)` from
// `PlayerInner::drop` removes it.
//
// Playback state flows back through a single `statusChanged` event whose
// payload carries `playerId`, so the owning Rust runtime can route it
// to the matching handle.

package rs.whisker.modules.audio

import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import rs.whisker.runtime.RuntimeAttachedListener
import rs.whisker.runtime.Module
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue


@WhiskerModule
class AudioModule : Module() {

    /**
     * Live players, keyed by the id the Rust side allocates. A plain
     * `HashMap` suffices because every lookup is on the main thread —
     * the Host bridge callback thread is the UI thread on Android.
     */
    private val players: MutableMap<Long, ExoPlayer> = mutableMapOf()
    private val loopFlags: MutableMap<Long, Boolean> = mutableMapOf()

    /**
     * `create` requests that arrived before any `WhiskerView` was attached
     * as a host. Rust's `Player::new` fires from the first render, which
     * runs inside `WhiskerView`'s constructor, so `currentActivity` is
     * still `null` then. Drained by the [RuntimeAttachedListener].
     */
    private val pendingCreates: MutableList<Pair<Long, String>> = mutableListOf()

    /**
     * Installed the first time [pendingCreates] has to queue something,
     * then kept for the process lifetime so the queue drains on every
     * re-attach.
     */
    private var hostListener: RuntimeAttachedListener? = null

    /**
     * Per-player position timer, ticking at ~200 ms while playing so the
     * Rust signal sees smooth progress. Cancelled on pause / stop /
     * release so a paused player doesn't pin a Handler post for nothing.
     */
    private val positionTicker = Handler(Looper.getMainLooper())
    private val tickRunnables: MutableMap<Long, Runnable> = mutableMapOf()

    override fun definition() = ModuleDefinition {
        Name("WhiskerAudio")
        Events("statusChanged")

        Function("create") { args ->
            val id = args.getOrNull(0)?.asInt() ?: return@Function WhiskerValue.Null
            val source = args.getOrNull(1)?.asString() ?: ""
            createPlayer(id, source)
            WhiskerValue.Null
        }
        Function("setSource") { args ->
            val id = args.getOrNull(0)?.asInt() ?: return@Function WhiskerValue.Null
            val source = args.getOrNull(1)?.asString() ?: ""
            players[id]?.let { p ->
                p.setMediaItem(MediaItem.fromUri(source))
                p.prepare()
            }
            WhiskerValue.Null
        }
        Function("play") { args ->
            args.getOrNull(0)?.asInt()?.let { players[it]?.play() }
            WhiskerValue.Null
        }
        Function("pause") { args ->
            args.getOrNull(0)?.asInt()?.let { players[it]?.pause() }
            WhiskerValue.Null
        }
        Function("stop") { args ->
            args.getOrNull(0)?.asInt()?.let { id ->
                players[id]?.let { p ->
                    p.pause()
                    p.seekTo(0)
                }
            }
            WhiskerValue.Null
        }
        Function("seekTo") { args ->
            val id = args.getOrNull(0)?.asInt() ?: return@Function WhiskerValue.Null
            val seconds = args.getOrNull(1)?.asDouble() ?: 0.0
            players[id]?.seekTo((seconds * 1000.0).toLong())
            WhiskerValue.Null
        }
        Function("setVolume") { args ->
            val id = args.getOrNull(0)?.asInt() ?: return@Function WhiskerValue.Null
            val v = (args.getOrNull(1)?.asDouble() ?: 1.0).toFloat().coerceIn(0f, 1f)
            players[id]?.volume = v
            WhiskerValue.Null
        }
        Function("setLoop") { args ->
            val id = args.getOrNull(0)?.asInt() ?: return@Function WhiskerValue.Null
            val looping = args.getOrNull(1)?.asBool() ?: false
            loopFlags[id] = looping
            players[id]?.repeatMode =
                if (looping) Player.REPEAT_MODE_ONE else Player.REPEAT_MODE_OFF
            WhiskerValue.Null
        }
        Function("release") { args ->
            args.getOrNull(0)?.asInt()?.let { id ->
                tickRunnables.remove(id)?.let { positionTicker.removeCallbacks(it) }
                players.remove(id)?.release()
                loopFlags.remove(id)
            }
            WhiskerValue.Null
        }
    }

    /**
     * Create the ExoPlayer for `id` and wire its `Player.Listener` so state
     * changes flow back to Rust. Defers when the host Activity isn't
     * attached yet — the common case during the very first render.
     */
    private fun createPlayer(id: Long, source: String) {
        val ctx = appContext.currentActivity
        if (ctx == null) {
            pendingCreates.add(id to source)
            ensureAttachListener()
            return
        }
        val p = ExoPlayer.Builder(ctx).build()
        if (source.isNotEmpty()) {
            p.setMediaItem(MediaItem.fromUri(source))
            p.prepare()
        }
        p.addListener(object : Player.Listener {
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                dispatchStatus(id)
                if (isPlaying) startTicker(id) else stopTicker(id)
            }
            override fun onPlaybackStateChanged(state: Int) {
                dispatchStatus(id)
            }
            override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
                dispatchStatus(id)
            }
        })
        players[id] = p
        dispatchStatus(id)
    }

    /**
     * Snapshot player `id` and broadcast it as `statusChanged`. No-ops when
     * the player is gone from the map — a listener callback can still land
     * after `release`.
     */
    private fun dispatchStatus(id: Long) {
        val p = players[id] ?: return
        val durationMs = p.duration
        val payload = mapOf(
            "playerId" to WhiskerValue.Int(id),
            "position" to WhiskerValue.Float(p.currentPosition / 1000.0),
            "duration" to WhiskerValue.Float(
                if (durationMs == C.TIME_UNSET) 0.0 else durationMs / 1000.0
            ),
            "isLoaded" to WhiskerValue.Bool(
                p.playbackState == Player.STATE_READY ||
                p.playbackState == Player.STATE_BUFFERING
            ),
            "isPlaying" to WhiskerValue.Bool(p.isPlaying),
        )
        sendEvent("statusChanged", WhiskerValue.Map(payload))
    }

    /**
     * The Handler runs on the main thread, the only one allowed to touch
     * ExoPlayer state.
     */
    private fun startTicker(id: Long) {
        // A rapid pause-play flip would otherwise stack two simultaneous
        // post chains.
        tickRunnables.remove(id)?.let { positionTicker.removeCallbacks(it) }
        val runnable = object : Runnable {
            override fun run() {
                if (players[id] == null) return
                dispatchStatus(id)
                positionTicker.postDelayed(this, 200L)
            }
        }
        tickRunnables[id] = runnable
        positionTicker.post(runnable)
    }

    private fun stopTicker(id: Long) {
        tickRunnables.remove(id)?.let { positionTicker.removeCallbacks(it) }
    }

    /**
     * One-shot install of the listener that drains [pendingCreates].
     * `addOnRuntimeAttachedListener` fires synchronously when a host is
     * already attached, so a late call still lands correctly.
     */
    private fun ensureAttachListener() {
        if (hostListener != null) return
        val listener = RuntimeAttachedListener {
            // Snapshot then clear, so a re-attach mid-drain (rotation)
            // can't see a half-drained queue.
            val pending = pendingCreates.toList()
            pendingCreates.clear()
            for ((id, source) in pending) {
                if (!players.containsKey(id)) {
                    createPlayer(id, source)
                }
            }
        }
        hostListener = listener
        appContext.addOnRuntimeAttachedListener(listener)
    }
}
