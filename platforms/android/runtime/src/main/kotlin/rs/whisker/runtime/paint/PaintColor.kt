package rs.whisker.runtime.paint

import android.graphics.Color

internal fun rgba(red: Float, green: Float, blue: Float, alpha: Float): Int = Color.argb(
    (alpha * 255f).toInt().coerceIn(0, 255),
    red.toInt().coerceIn(0, 255),
    green.toInt().coerceIn(0, 255),
    blue.toInt().coerceIn(0, 255),
)

internal fun parseNamedColor(name: String): Int =
    when (name.lowercase()) {
        "gold" -> Color.rgb(255, 215, 0)
        // Android's named `green` is #00ff00, while CSS defines `green` as
        // #008000. Keep Host paint color resolution aligned with CSS/WPT.
        "green" -> Color.rgb(0, 128, 0)
        "transparent" -> Color.TRANSPARENT
        else -> runCatching { Color.parseColor(name) }.getOrDefault(Color.TRANSPARENT)
    }
