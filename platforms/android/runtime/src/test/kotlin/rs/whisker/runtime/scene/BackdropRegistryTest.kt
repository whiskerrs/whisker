package rs.whisker.runtime.scene

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BackdropRegistryTest {
    private class Node(val parent: Node?)

    private fun registry() = BackdropRegistry<Node, Node> { node, target ->
        generateSequence(target) { it.parent }.any { it === node }
    }

    private val root = Node(null)
    private val underScreen = Node(root)
    private val topScreen = Node(root)
    private val backdrop = Node(topScreen)
    private val backdropChild = Node(backdrop)

    @Test
    fun `a moving ancestor or repainting sibling re-records the backdrop`() {
        val registry = registry()
        registry.update(backdrop, active = true)

        assertEquals(listOf(backdrop), registry.affectedBy(topScreen))
        assertEquals(listOf(backdrop), registry.affectedBy(underScreen))
    }

    @Test
    fun `the backdrop's own subtree does not re-record it`() {
        val registry = registry()
        registry.update(backdrop, active = true)

        assertTrue(registry.affectedBy(backdrop).isEmpty())
        assertTrue(registry.affectedBy(backdropChild).isEmpty())
    }

    @Test
    fun `an inactive backdrop is forgotten`() {
        val registry = registry()
        registry.update(backdrop, active = true)
        registry.update(backdrop, active = false)

        assertTrue(registry.isEmpty)
        assertTrue(registry.affectedBy(underScreen).isEmpty())
    }
}
