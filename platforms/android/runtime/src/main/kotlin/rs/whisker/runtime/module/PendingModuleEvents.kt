package rs.whisker.runtime.module

import rs.whisker.runtime.WhiskerValue

internal data class PendingModuleEvent(
    val epoch: Long,
    val module: String,
    val event: String,
    val payload: WhiskerValue,
)

/** Any-thread ingress queue drained once per main-loop turn. */
internal class PendingModuleEvents {
    private val lock = Any()
    private val queue = ArrayDeque<PendingModuleEvent>()
    private var flushScheduled = false

    /** Returns true only when the caller must schedule the shared flush. */
    fun offer(event: PendingModuleEvent): Boolean = synchronized(lock) {
        queue.addLast(event)
        if (flushScheduled) {
            false
        } else {
            flushScheduled = true
            true
        }
    }

    fun drain(epoch: Long): List<PendingModuleEvent> = synchronized(lock) {
        flushScheduled = false
        buildList {
            while (queue.isNotEmpty()) {
                queue.removeFirst().takeIf { it.epoch == epoch }?.let(::add)
            }
        }
    }

    fun clear() = synchronized(lock) {
        queue.clear()
        flushScheduled = false
    }
}
