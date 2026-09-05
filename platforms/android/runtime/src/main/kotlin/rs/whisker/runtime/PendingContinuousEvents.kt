package rs.whisker.runtime

/** One Host event sample waiting for the current Android draw to complete. */
internal data class PendingElementEvent(
    val node: Long,
    val name: String,
    val detail: WhiskerValue,
    val timestampMs: Double,
)

/**
 * Coalesces movement samples per element without losing drag transitions.
 *
 * Android can report several scroll offsets while preparing one frame.
 * Keeping the newest position avoids reconciling List for every native sample,
 * but release velocity and drag boundaries must reach Rust in order.
 */
internal class PendingContinuousEvents {
    private val events = mutableListOf<PendingElementEvent>()
    private val latest = mutableMapOf<Pair<Long, String>, Int>()

    fun offer(event: PendingElementEvent) {
        val key = event.node to event.name
        val index = latest[key]
        val previous = index?.let { events[it] }
        val old = (previous?.detail as? WhiskerValue.Map)?.value
        val next = (event.detail as? WhiskerValue.Map)?.value
        fun isRelease(detail: Map<String, WhiskerValue>?): Boolean =
            detail?.get("isDragCancelled") == WhiskerValue.Bool(true) ||
                listOf("velocityX", "velocityY").any {
                    ((detail?.get(it) as? WhiskerValue.Float)?.value ?: 0.0) != 0.0
                }

        if (index != null && old?.get("isDragging") == next?.get("isDragging") &&
            !isRelease(old) && !isRelease(next)
        ) {
            // Deltas describe movement since the last event Rust received, not
            // just the last native sample retained in this frame.
            val detail = if (old != null && next != null) {
                val merged = next.toMutableMap()
                for (axis in listOf("deltaX", "deltaY")) {
                    if (axis in old || axis in next) {
                        merged[axis] = WhiskerValue.Float(
                            ((old[axis] as? WhiskerValue.Float)?.value ?: 0.0) +
                                ((next[axis] as? WhiskerValue.Float)?.value ?: 0.0),
                        )
                    }
                }
                WhiskerValue.Map(merged)
            } else event.detail
            events[index] = event.copy(detail = detail)
        } else {
            latest[key] = events.size
            events.add(event)
        }
    }

    fun drain(): List<PendingElementEvent> {
        if (events.isEmpty()) return emptyList()
        val drained = events.toList()
        clear()
        return drained
    }

    fun clear() {
        events.clear()
        latest.clear()
    }
}
