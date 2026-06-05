// `whisker-audio` Module (iOS). View-less.
//
// Mirrors the Android side: a `[Int64: AVPlayer]` keyed on the
// Rust-allocated id, prop-style functions (`create`, `play`,
// `pause`, `setLoop`, etc.), and a `statusChanged` event whose
// payload carries `{ playerId, position, duration, isLoaded,
// isPlaying }`.
//
// AVPlayer has no built-in loop / status callback API equivalent
// to Media3's `Player.Listener` — looping rides on
// `AVPlayerItemDidPlayToEndTime`, and status updates ride on
// `KVO` (`status`, `rate`) + a periodic time observer at
// ~200 ms cadence.

import AVFoundation
import Foundation
import WhiskerModule
import WhiskerRuntime

public final class AudioModule: Module {

    /// Per-player state. Owned by the module, indexed by the id
    /// the Rust side allocates on `Player::new`.
    private final class PlayerEntry {
        let player: AVPlayer
        var loop: Bool = false
        var endObserver: NSObjectProtocol?
        var statusObservation: NSKeyValueObservation?
        var rateObservation: NSKeyValueObservation?
        /// Foundation Timer driving position dispatch while
        /// playback is active. `addPeriodicTimeObserver` was
        /// unreliable on iOS Simulator (token returned non-nil but
        /// the closure never fired), so we hand-tick instead.
        var progressTimer: Timer?

        init(player: AVPlayer) {
            self.player = player
        }
    }

    private var players: [Int64: PlayerEntry] = [:]

    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("WhiskerAudio")
            Events("statusChanged")

            Function("create") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let id = args.first?.asInt else { return .null }
                let source = args.count > 1 ? (args[1].asString ?? "") : ""
                self.createPlayer(id: id, source: source)
                return .null
            }
            Function("setSource") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let id = args.first?.asInt,
                      let entry = self.players[id] else { return .null }
                let source = args.count > 1 ? (args[1].asString ?? "") : ""
                self.swapItem(for: entry, id: id, source: source)
                return .null
            }
            Function("play") { (args: [WhiskerValue]) -> WhiskerValue in
                if let id = args.first?.asInt { self.players[id]?.player.play() }
                return .null
            }
            Function("pause") { (args: [WhiskerValue]) -> WhiskerValue in
                if let id = args.first?.asInt { self.players[id]?.player.pause() }
                return .null
            }
            Function("stop") { (args: [WhiskerValue]) -> WhiskerValue in
                if let id = args.first?.asInt, let entry = self.players[id] {
                    entry.player.pause()
                    entry.player.seek(to: .zero)
                }
                return .null
            }
            Function("seekTo") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let id = args.first?.asInt, let entry = self.players[id] else {
                    return .null
                }
                let seconds = args.count > 1 ? (args[1].asDouble ?? 0) : 0
                let time = CMTime(seconds: seconds, preferredTimescale: 600)
                entry.player.seek(to: time)
                return .null
            }
            Function("setVolume") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let id = args.first?.asInt, let entry = self.players[id] else {
                    return .null
                }
                let raw = args.count > 1 ? (args[1].asDouble ?? 1.0) : 1.0
                entry.player.volume = Float(max(0, min(1, raw)))
                return .null
            }
            Function("setLoop") { (args: [WhiskerValue]) -> WhiskerValue in
                guard let id = args.first?.asInt, let entry = self.players[id] else {
                    return .null
                }
                let looping = args.count > 1 ? (args[1].asBool ?? false) : false
                entry.loop = looping
                self.installEndObserver(for: entry, id: id)
                return .null
            }
            Function("release") { (args: [WhiskerValue]) -> WhiskerValue in
                if let id = args.first?.asInt { self.releasePlayer(id: id) }
                return .null
            }
        }
    }

    // MARK: - Player lifecycle

    private func createPlayer(id: Int64, source: String) {
        // iOS audio output is gated on an active AVAudioSession.
        // Without `.playback` + `setActive(true)`, AVPlayer reports
        // rate=1 after `play()` but the audio render loop never
        // pulls samples — currentTime stays at 0 and no audio
        // reaches the speaker. Activating the session is
        // idempotent across players; we re-call on every create
        // so a new player picks up the session even if the host
        // app deactivated it.
        ensureAudioSessionActive()

        let player: AVPlayer
        if !source.isEmpty, let url = URL(string: source) {
            player = AVPlayer(url: url)
        } else {
            player = AVPlayer()
        }
        let entry = PlayerEntry(player: player)
        players[id] = entry
        installKVO(for: entry, id: id)
        installEndObserver(for: entry, id: id)
        dispatchStatus(id: id)
    }

    private func swapItem(for entry: PlayerEntry, id: Int64, source: String) {
        if source.isEmpty {
            entry.player.replaceCurrentItem(with: nil)
        } else if let url = URL(string: source) {
            entry.player.replaceCurrentItem(with: AVPlayerItem(url: url))
        }
        // Loop observer is item-scoped; reinstall on the fresh
        // playerItem so end-of-source seeking still fires when
        // appropriate.
        installEndObserver(for: entry, id: id)
        dispatchStatus(id: id)
    }

    private func releasePlayer(id: Int64) {
        guard let entry = players.removeValue(forKey: id) else { return }
        entry.progressTimer?.invalidate()
        if let token = entry.endObserver {
            NotificationCenter.default.removeObserver(token)
        }
        entry.statusObservation?.invalidate()
        entry.rateObservation?.invalidate()
        entry.player.pause()
        entry.player.replaceCurrentItem(with: nil)
    }

    // MARK: - Observation

    /// Re-broadcast `statusChanged` whenever the AVPlayer's loaded
    /// state or play / pause flag transitions. The rate observer
    /// also starts / stops the position-progress Timer so a
    /// paused player doesn't pin a runloop callback.
    private func installKVO(for entry: PlayerEntry, id: Int64) {
        entry.rateObservation = entry.player.observe(\.rate, options: [.new]) { [weak self] player, _ in
            self?.dispatchStatus(id: id)
            guard let self else { return }
            if player.rate > 0 {
                self.startProgressTimer(for: id)
            } else {
                self.stopProgressTimer(for: id)
            }
        }
        // `status` lives on AVPlayerItem; observe transitions
        // through replaceCurrentItem too — KVO continues across
        // item swaps because we re-look up `.currentItem` inside
        // the closure on every fire.
        entry.statusObservation = entry.player.observe(\.currentItem?.status, options: [.new]) { [weak self] _, _ in
            self?.dispatchStatus(id: id)
        }
    }

    /// Start (or restart) a ~200 ms Timer that re-broadcasts
    /// playback status while the player rate is non-zero.
    ///
    /// We use a hand-rolled Timer instead of
    /// `AVPlayer.addPeriodicTimeObserver` because the latter
    /// silently never fires on iOS Simulator (token comes back
    /// non-nil; the closure is never invoked). Real devices honour
    /// the periodic observer correctly, but the dev loop runs on
    /// the sim so a portable solution wins.
    private func startProgressTimer(for id: Int64) {
        guard let entry = players[id] else { return }
        entry.progressTimer?.invalidate()
        let timer = Timer.scheduledTimer(withTimeInterval: 0.2, repeats: true) { [weak self] _ in
            self?.dispatchStatus(id: id)
        }
        // Also schedule on `.common` modes so a touch-driven runloop
        // mode (e.g. tracking a scroll) doesn't pause the tick.
        RunLoop.main.add(timer, forMode: .common)
        entry.progressTimer = timer
    }

    private func stopProgressTimer(for id: Int64) {
        players[id]?.progressTimer?.invalidate()
        players[id]?.progressTimer = nil
    }

    /// Activate a `.playback` AVAudioSession so AVPlayer's render
    /// loop actually pulls samples. Whisker apps don't currently
    /// take a stance on session category (no recording, mixing,
    /// etc), so `.playback` is the safe default — it solo-routes
    /// audio to the device speaker and survives a screen lock.
    private func ensureAudioSessionActive() {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playback, mode: .default)
            try session.setActive(true, options: [])
        } catch {
            NSLog("[whisker-audio] AVAudioSession activate failed: \(error)")
        }
    }

    /// (Re-)install the loop-on-end observer. Removes the prior
    /// token before adding a fresh one so a sequence of
    /// `setLoop(true)` / `setLoop(false)` / `setLoop(true)` calls
    /// doesn't stack handlers.
    private func installEndObserver(for entry: PlayerEntry, id: Int64) {
        if let token = entry.endObserver {
            NotificationCenter.default.removeObserver(token)
            entry.endObserver = nil
        }
        guard entry.loop, let item = entry.player.currentItem else { return }
        entry.endObserver = NotificationCenter.default.addObserver(
            forName: .AVPlayerItemDidPlayToEndTime,
            object: item,
            queue: .main
        ) { [weak self] _ in
            self?.players[id]?.player.seek(to: .zero)
            self?.players[id]?.player.play()
        }
    }

    // MARK: - Status payload

    private func dispatchStatus(id: Int64) {
        guard let entry = players[id] else { return }
        let player = entry.player
        let position = CMTimeGetSeconds(player.currentTime())
        let duration = player.currentItem.map { CMTimeGetSeconds($0.duration) } ?? 0
        let safeDuration = duration.isFinite ? duration : 0
        // `currentItem.status == .readyToPlay` is the canonical
        // "loaded" signal, but iOS Simulator hides the
        // transition behind a KVO event that doesn't always
        // fire — derive from "we have a finite duration" instead
        // so the flag flips as soon as the asset metadata
        // resolves, regardless of the status KVO path.
        let isLoaded = safeDuration > 0 || (player.currentItem?.status == .readyToPlay)
        let isPlaying = player.rate > 0 && (player.error == nil)
        let payload: WhiskerValue = .map([
            "playerId": .int(id),
            "position": .float(position.isFinite ? position : 0),
            "duration": .float(safeDuration),
            "isLoaded": .bool(isLoaded),
            "isPlaying": .bool(isPlaying),
        ])
        sendEvent("statusChanged", payload)
    }
}
