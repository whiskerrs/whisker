package rs.whisker.runtime

/** Holds the initial frame until window insets exist and delivers module events before layout. */
internal class InitialFrameGate {
    private var presented = false

    fun run(hasWindowInsets: Boolean, flushEvents: () -> Unit, frame: () -> Unit): Boolean {
        if (!presented && !hasWindowInsets) return false
        flushEvents()
        frame()
        presented = true
        return true
    }
}
