package rs.whisker.runtime.scene

import org.junit.Assert.*
import org.junit.Test

class SceneTopologyTest {
    @Test fun removingSubtreePreservesDetachedAndUnrelatedNodes() {
        val tree = SceneTopology()
        (1L..8L).forEach { assertTrue(tree.add(it)) }
        tree.attach(1, 2)
        tree.attach(2, 3)
        tree.attach(2, 4)
        tree.attach(4, 5)
        tree.attach(6, 7)
        assertEquals(2L, tree.detach(4))
        assertEquals(setOf(2L, 3L), tree.removeSubtree(2).toSet())
        assertEquals(0, tree.childCount(1))
        assertNull(tree.parentOf(4))
        assertEquals(4L, tree.parentOf(5))
        assertEquals(6L, tree.parentOf(7))
        tree.attach(6, 4)
        assertEquals(setOf(6L, 7L, 4L, 5L), tree.removeSubtree(6).toSet())
        assertTrue(1L in tree)
        assertTrue(8L in tree)
        assertTrue(tree.removeSubtree(6).isEmpty())
    }

    @Test fun stagedChangesAreIsolatedFromTheLiveTree() {
        val live = SceneTopology()
        (1L..4L).forEach { live.add(it) }
        live.attach(1, 2)
        live.attach(2, 3)
        val staged = live.copy()
        staged.detach(2)
        staged.attach(4, 2)
        staged.removeSubtree(4)
        assertEquals(1L, live.parentOf(2))
        assertEquals(2L, live.parentOf(3))
        assertEquals(setOf(1L, 2L), live.parentIds())
        assertEquals(1, live.childCount(1))
        assertTrue(4L in live)
        assertTrue(1L in staged)
    }

    @Test fun deepTreesCanBeRemovedWithoutRecursion() {
        val tree = SceneTopology()
        val depth = 20_000L
        (1L..depth).forEach { tree.add(it) }
        // Attach from leaves to keep setup linear too.
        (depth downTo 2L).forEach { tree.attach(it - 1, it) }
        assertEquals(depth.toInt(), tree.removeSubtree(1).size)
        assertTrue(tree.parentIds().isEmpty())
        assertTrue(tree.add(1))
        assertFalse(tree.add(1))
    }

    @Test fun cyclesAndDoubleAttachmentsCannotCorruptTheTree() {
        val tree = SceneTopology()
        (1L..3L).forEach { tree.add(it) }
        tree.attach(1, 2)
        tree.attach(2, 3)
        assertThrows(IllegalStateException::class.java) { tree.attach(3, 1) }
        assertThrows(IllegalStateException::class.java) { tree.attach(1, 3) }
        assertThrows(IllegalStateException::class.java) { tree.attach(4, 1) }
        assertTrue(tree.isDescendantOrSelf(3, 1))
        assertEquals(setOf(1L, 2L, 3L), tree.removeSubtree(1).toSet())
        tree.add(4)
        tree.clear()
        assertFalse(4L in tree)
    }
}
