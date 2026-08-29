package rs.whisker.runtime

/** One Host event sample waiting for the current Android draw to complete. */
internal data class PendingElementEvent(
    val node: Long,
    val name: String,
    val detail: WhiskerValue,
    val timestampMs: Double,
)

/**
 * Keeps only the newest continuous-event sample for each element.
 *
 * Android can report several scroll offsets while the UI thread is preparing
 * one frame. Rust only needs the newest offset to select the next List window;
 * synchronously reconciling every intermediate sample stalls ScrollView's own
 * input handling.
 */
internal class PendingContinuousEvents {
    private val events = LinkedHashMap<Pair<Long, String>, PendingElementEvent>()

    fun offer(event: PendingElementEvent) {
        events[event.node to event.name] = event
    }

    fun drain(): List<PendingElementEvent> {
        if (events.isEmpty()) return emptyList()
        val drained = events.values.toList()
        events.clear()
        return drained
    }

    fun clear() {
        events.clear()
    }
}
