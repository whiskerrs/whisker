package rs.whisker.runtime.measure

import android.text.StaticLayout
import java.lang.ref.ReferenceQueue
import java.lang.ref.WeakReference

internal class PreparedParagraphs {
    private class Entry(val id: Long, value: StaticLayout, queue: ReferenceQueue<StaticLayout>) : WeakReference<StaticLayout>(value, queue)
    private val queue = ReferenceQueue<StaticLayout>()
    private val entries = HashMap<Long, Entry>()

    fun put(id: Long, layout: StaticLayout) {
        drain()
        entries[id] = Entry(id, layout, queue)
    }

    fun get(id: Long): StaticLayout? {
        drain()
        return entries[id]?.get()
    }

    private fun drain() {
        while (true) {
            val entry = queue.poll() as? Entry ?: return
            entries.remove(entry.id, entry)
        }
    }
}
