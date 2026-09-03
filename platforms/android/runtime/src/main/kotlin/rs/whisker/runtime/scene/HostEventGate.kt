package rs.whisker.runtime.scene

/** Prevents Host element callbacks from re-entering Rust while a frame is being presented. */
internal class HostEventGate(
    private val schedule: (() -> Unit) -> Unit,
) {
    private var isApplyingFrame = false
    private val deferred = ArrayList<() -> Unit>()

    fun beginFrame() {
        check(!isApplyingFrame)
        isApplyingFrame = true
    }

    fun endFrame() {
        check(isApplyingFrame)
        isApplyingFrame = false
        if (deferred.isEmpty()) return
        val events = deferred.toList()
        deferred.clear()
        schedule {
            events.forEach { it() }
        }
    }

    fun dispatch(event: () -> Unit) {
        if (isApplyingFrame) deferred += event else event()
    }
}
