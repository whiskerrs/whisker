package rs.whisker.runtime.paint

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BackgroundLayersTest {
    private val center = HostPaintCoordinate(length = 0f, fraction = 0.5f)
    private val zero = HostPaintCoordinate(length = 0f, fraction = 0f)

    @Test
    fun `farthest corner resolves circle and ellipse against the image box`() {
        val circle = resolveRadialRadii(
            200f,
            100f,
            gradient(HostRadialShape.Circle, HostRadialExtent.FarthestCorner),
        )
        assertEquals(circle.x, circle.y, 0f)
        assertEquals(111.8034f, circle.x, 0.001f)

        val ellipse = resolveRadialRadii(
            200f,
            100f,
            gradient(HostRadialShape.Ellipse, HostRadialExtent.FarthestCorner),
        )
        assertEquals(141.42136f, ellipse.x, 0.001f)
        assertEquals(70.71068f, ellipse.y, 0.001f)
    }

    @Test
    fun `explicit circle uses one radius on both axes`() {
        val radius = HostPaintCoordinate(length = 40f, fraction = 0f)
        val radii = resolveRadialRadii(
            200f,
            100f,
            HostRadialGradient(
                shape = HostRadialShape.Circle,
                extent = HostRadialExtent.Explicit,
                centerX = center,
                centerY = center,
                radiusX = radius,
                radiusY = zero,
                stops = emptyList(),
            ),
        )
        assertTrue(radii.x > 0f)
        assertEquals(40f, radii.x, 0f)
        assertEquals(40f, radii.y, 0f)
    }

    private fun gradient(shape: HostRadialShape, extent: HostRadialExtent) = HostRadialGradient(
        shape = shape,
        extent = extent,
        centerX = center,
        centerY = center,
        radiusX = zero,
        radiusY = zero,
        stops = emptyList(),
    )
}
