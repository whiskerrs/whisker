// `whisker-audio` Module (Android). View-less.
//
// Holds a `MutableMap<Long, ExoPlayer>` of active players; each
// Rust-side `Player` allocation calls `create(id, source)` to
// install a fresh entry, drives it through `play` / `pause` /
// `seekTo` / etc, and `release(id)` from `PlayerInner::drop`
// removes it.
//
// Per-player playback state is dispatched back through a
// `statusChanged` event whose payload carries the `playerId`
// alongside the position / duration / flags — the Rust side's
// global dispatch table routes each event to the matching handle.

package rs.whisker.modules.audio

import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import rs.whisker.runtime.HostAttachedListener
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerValue

class AudioModule : Module() {

    /**
     * Live players, keyed by the id the Rust side allocates. Map
     * lookups happen on the main thread (Lynx's bridge dispatch
     * thread is the UI thread on Android), so a plain
     * `HashMap` is enough — no Concurrent variant needed.
     */
    private val players: MutableMap<Long, ExoPlayer> = mutableMapOf()
    private val loopFlags: MutableMap<Long, Boolean> = mutableMapOf()

    /**
     * Pending `create` requests that arrived before any
     * `WhiskerView` was attached as a host. `Player::new` on the
     * Rust side fires from the first render, which runs inside
     * `WhiskerView`'s constructor — `currentActivity` is still
     * `null` at that moment. We stash the requests here and drain
     * them from the [HostAttachedListener] body.
     */
    private val pendingCreates: MutableList<Pair<Long, String>> = mutableListOf()

    /**
     * `null` until [ensureAttachListener] runs the first time
     * `pendingCreates` had to queue something; from then on it
     * stays installed for the process lifetime and drains the
     * queue on every (re-)attach.
     */
    private var hostListener: HostAttachedListener? = null

    /**
     * Per-player position timer. While `isPlaying` we tick at
     * ~200 ms cadence so the Rust signal sees smooth progress;
     * the timer is cancelled on `pause` / `stop` / `release` so a
     * paused player doesn't pin a Handler post for nothing.
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
     * Create the ExoPlayer instance for `id` and wire its
     * `Player.Listener` so state changes flow back to Rust via
     * `statusChanged` events.
     *
     * Defers if the host Activity isn't attached yet (the common
     * case when `Player::new` runs during the very first render).
     * Queued requests drain via [ensureAttachListener] once the
     * WhiskerView attaches.
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
     * Snapshot the current state of player `id` and broadcast it
     * as a `statusChanged` event. Silently no-ops if the player
     * is no longer in the map (e.g. event fired after release).
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
     * Begin posting a status update every ~200 ms for player `id`.
     * The Handler runs on the main thread, the only one allowed to
     * touch ExoPlayer state.
     */
    private fun startTicker(id: Long) {
        // Cancel any prior ticker before installing a fresh one —
        // a rapid pause-play flip otherwise stacks two
        // simultaneous post chains.
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
     * One-shot install of the host-attached listener that drains
     * [pendingCreates]. `addOnHostAttachedListener` fires
     * synchronously if a host is already attached, so even a late
     * call after host-attach lands at the right place.
     */
    private fun ensureAttachListener() {
        if (hostListener != null) return
        val listener = HostAttachedListener {
            // Snapshot then clear so a re-attach during drain
            // (e.g. config-change rotation) doesn't see a
            // half-drained queue.
            val pending = pendingCreates.toList()
            pendingCreates.clear()
            for ((id, source) in pending) {
                if (!players.containsKey(id)) {
                    createPlayer(id, source)
                }
            }
        }
        hostListener = listener
        appContext.addOnHostAttachedListener(listener)
    }
}
