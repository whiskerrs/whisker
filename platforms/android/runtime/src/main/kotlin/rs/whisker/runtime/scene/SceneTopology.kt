package rs.whisker.runtime.scene

import java.util.ArrayDeque

internal class SceneTopology private constructor(
    private val parents: HashMap<Long, Long>,
    private val children: HashMap<Long, LinkedHashSet<Long>>,
) {
    constructor() : this(HashMap(), HashMap())

    fun copy(): SceneTopology = SceneTopology(
        HashMap(parents),
        HashMap(children.mapValues { LinkedHashSet(it.value) }),
    )

    operator fun contains(node: Long): Boolean = children.containsKey(node)

    fun add(node: Long): Boolean {
        if (node in this) return false
        children[node] = LinkedHashSet()
        return true
    }

    fun parentOf(node: Long): Long? = parents[node]

    fun parentIds(): Set<Long> = parents.values.toSet()

    fun childCount(node: Long): Int = children[node]?.size ?: 0

    fun isDescendantOrSelf(candidate: Long, ancestor: Long): Boolean {
        var current: Long? = candidate
        while (current != null) {
            if (current == ancestor) return true
            current = parents[current]
        }
        return false
    }

    fun attach(parent: Long, child: Long) {
        check(parent in this && child in this)
        check(parentOf(child) == null && !isDescendantOrSelf(parent, child))
        children.getValue(parent).add(child)
        parents[child] = parent
    }

    fun detach(child: Long): Long? {
        val parent = parents.remove(child) ?: return null
        children.getValue(parent).remove(child)
        return parent
    }

    fun removeSubtree(node: Long): List<Long> {
        if (node !in this) return emptyList()
        detach(node)
        val removed = ArrayList<Long>()
        val pending = ArrayDeque<Long>()
        pending.add(node)
        while (pending.isNotEmpty()) {
            val current = pending.removeLast()
            pending.addAll(children.remove(current)!!)
            parents.remove(current)
            removed.add(current)
        }
        return removed
    }

    fun clear() {
        parents.clear()
        children.clear()
    }
}
