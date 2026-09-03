package rs.whisker.runtime

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class WhiskerElementBindingsTest {
    @Test
    fun bindingASecondSurfaceDoesNotReplaceTheFirstSurfacesIds() {
        val name = "binding-test/Independent"
        WhiskerElementRegistry.register(
            WhiskerElementFactory(name = name) { error("mount is not needed by this test") },
        )
        fun registration(elementType: Int) = WhiskerElementRegistration(
            elementType = elementType,
            name = name,
            childPolicy = WhiskerChildPolicy.None,
            measurement = WhiskerMeasurement.None,
        )
        val first = WhiskerElementRegistry.newBindings()
        val second = WhiskerElementRegistry.newBindings()

        assertTrue(WhiskerElementRegistry.bind(first, listOf(registration(11))))
        assertTrue(WhiskerElementRegistry.bind(second, listOf(registration(29))))

        assertEquals(11, first.registration(11)?.elementType)
        assertNull(first.registration(29))
        assertEquals(29, second.registration(29)?.elementType)
        assertNull(second.registration(11))
    }
}
