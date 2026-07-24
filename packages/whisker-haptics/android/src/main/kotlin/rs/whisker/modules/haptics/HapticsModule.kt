// `whisker-haptics` ModuleDefinition (Android).
//
// A view-less DSL module: `definition()` has no `View(...)` block, just
// module-level `Function`s. The KSP processor finds the `Module`
// subclass and registers its functions with `WhiskerModuleRegistry`
// under the `Name(...)`, so `whisker::platform_module::invoke(
// "WhiskerHaptics", ...)` from Rust routes into these handlers.
//
// Android has no predefined `VibrationEffect` mapping to
// success/warning/error the way iOS's `UINotificationFeedbackGenerator`
// does, so `notification` uses short waveform patterns instead — a
// single tap (success), one longer buzz (warning), or three short taps
// (error). SDK branching: `VibratorManager` needs API 31+,
// `VibrationEffect.createPredefined`/`createWaveform` need API
// 29+/26+ respectively; below that this falls back to the deprecated
// `Vibrator.vibrate(ms)` / `vibrate(LongArray, repeat)` overloads.

package rs.whisker.modules.haptics

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerApplication
import rs.whisker.runtime.WhiskerValue

class HapticsModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("WhiskerHaptics")

        // impact(style: "light" | "medium" | "heavy") -> Null | Err
        Function("impact") { args ->
            try {
                val style = args.getOrNull(0)?.asString() ?: "light"
                Haptics.impact(style)
                WhiskerValue.Null
            } catch (t: Throwable) {
                WhiskerValue.Err("WhiskerHaptics.impact failed: ${t.message}")
            }
        }

        // selection() -> Null | Err
        Function("selection") { _ ->
            try {
                Haptics.selection()
                WhiskerValue.Null
            } catch (t: Throwable) {
                WhiskerValue.Err("WhiskerHaptics.selection failed: ${t.message}")
            }
        }

        // notification(kind: "success" | "warning" | "error") -> Null | Err
        Function("notification") { args ->
            try {
                val kind = args.getOrNull(0)?.asString() ?: "success"
                Haptics.notification(kind)
                WhiskerValue.Null
            } catch (t: Throwable) {
                WhiskerValue.Err("WhiskerHaptics.notification failed: ${t.message}")
            }
        }
    }
}

/// Plain helper — no Whisker / Lynx types. Kept separate from
/// `HapticsModule` so the vibration logic is testable/readable on its
/// own, matching `whisker-pdf`'s `PdfModule`/`PdfRenderer` split.
private object Haptics {
    private fun context(): Context =
        WhiskerApplication.appContext
            ?: throw IllegalStateException(
                "WhiskerHaptics: WhiskerApplication.appContext not initialised — " +
                    "ensure your Application extends WhiskerApplication and " +
                    "super.onCreate() runs before any module dispatch",
            )

    private fun vibrator(): Vibrator {
        val ctx = context()
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val manager = ctx.getSystemService(Context.VIBRATOR_MANAGER_SERVICE) as VibratorManager
            manager.defaultVibrator
        } else {
            @Suppress("DEPRECATION")
            ctx.getSystemService(Context.VIBRATOR_SERVICE) as Vibrator
        }
    }

    private fun predefined(effectId: Int, fallbackMs: Long, fallbackAmplitude: Int) {
        val v = vibrator()
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q ->
                v.vibrate(VibrationEffect.createPredefined(effectId))
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.O ->
                v.vibrate(VibrationEffect.createOneShot(fallbackMs, fallbackAmplitude))
            else -> {
                @Suppress("DEPRECATION")
                v.vibrate(fallbackMs)
            }
        }
    }

    private fun waveform(timingsMs: LongArray, amplitudes: IntArray, fallbackPatternMs: LongArray) {
        val v = vibrator()
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.O ->
                v.vibrate(VibrationEffect.createWaveform(timingsMs, amplitudes, -1))
            else -> {
                @Suppress("DEPRECATION")
                v.vibrate(fallbackPatternMs, -1)
            }
        }
    }

    fun impact(style: String) {
        when (style) {
            "heavy" -> predefined(VibrationEffect.EFFECT_HEAVY_CLICK, fallbackMs = 30, fallbackAmplitude = 255)
            "medium" -> predefined(VibrationEffect.EFFECT_CLICK, fallbackMs = 20, fallbackAmplitude = 180)
            else -> predefined(VibrationEffect.EFFECT_TICK, fallbackMs = 10, fallbackAmplitude = 100)
        }
    }

    fun selection() {
        predefined(VibrationEffect.EFFECT_TICK, fallbackMs = 5, fallbackAmplitude = 80)
    }

    fun notification(kind: String) {
        when (kind) {
            "warning" -> waveform(
                timingsMs = longArrayOf(0, 60),
                amplitudes = intArrayOf(0, 180),
                fallbackPatternMs = longArrayOf(0, 60),
            )
            "error" -> waveform(
                timingsMs = longArrayOf(0, 40, 60, 40, 60, 40),
                amplitudes = intArrayOf(0, 255, 0, 255, 0, 255),
                fallbackPatternMs = longArrayOf(0, 40, 60, 40, 60, 40),
            )
            else -> waveform(
                timingsMs = longArrayOf(0, 20, 40, 20),
                amplitudes = intArrayOf(0, 150, 0, 220),
                fallbackPatternMs = longArrayOf(0, 20, 40, 20),
            )
        }
    }
}
