package rs.whisker.runtime.scene

/**
 * Tracks the nodes that paint a backdrop blur and decides which of them must re-record.
 *
 * A backdrop node bakes a snapshot of everything painted before it, plus its own offset from the
 * Whisker root, into its display list. Android only re-records that list when the node itself is
 * invalidated, so a change elsewhere (an ancestor sliding in, the screen beneath repainting) would
 * otherwise leave a stale, shifted blur on screen.
 */
internal class BackdropRegistry<N : Any, V : Any>(
    private val isSelfOrAncestorOf: (node: N, target: V) -> Boolean,
) {
    private val nodes = LinkedHashSet<N>()

    val isEmpty: Boolean
        get() = nodes.isEmpty()

    fun update(node: N, active: Boolean) {
        if (active) nodes.add(node) else nodes.remove(node)
    }

    /** Backdrop nodes whose snapshot may depend on [target]; its own subtree paints after it. */
    fun affectedBy(target: V): List<N> = nodes.filter { !isSelfOrAncestorOf(it, target) }
}
