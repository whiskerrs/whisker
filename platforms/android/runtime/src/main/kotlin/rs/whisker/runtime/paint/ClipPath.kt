package rs.whisker.runtime.paint

import android.graphics.Path
import android.graphics.RectF

internal enum class HostClipReferenceBox { Border, Padding, Content }

/** Rounded inset basic shape retained in physical Host coordinates. */
internal data class HostInsetClipPath(
    val referenceBox: HostClipReferenceBox,
    val edges: List<HostPaintCoordinate>,
    val radiiHorizontal: List<HostPaintCoordinate>,
    val radiiVertical: List<HostPaintCoordinate>,
)

internal fun resolveInsetClipPath(
    clip: HostInsetClipPath,
    width: Float,
    height: Float,
    borderWidths: FloatArray,
    contentBox: RectF,
): Path {
    val reference = when (clip.referenceBox) {
        HostClipReferenceBox.Border -> RectF(0f, 0f, width, height)
        HostClipReferenceBox.Padding -> RectF(
            borderWidths[3].coerceIn(0f, width),
            borderWidths[0].coerceIn(0f, height),
            width - borderWidths[1].coerceIn(0f, width),
            height - borderWidths[2].coerceIn(0f, height),
        )
        HostClipReferenceBox.Content -> RectF(contentBox)
    }
    val top = clip.edges[0].resolve(reference.height())
    val right = clip.edges[1].resolve(reference.width())
    val bottom = clip.edges[2].resolve(reference.height())
    val left = clip.edges[3].resolve(reference.width())
    val inset = RectF(
        reference.left + left,
        reference.top + top,
        (reference.right - right).coerceAtLeast(reference.left + left),
        (reference.bottom - bottom).coerceAtLeast(reference.top + top),
    )
    if (inset.isEmpty) return Path()
    val radii = normalizeRadii(
        floatArrayOf(
            clip.radiiHorizontal[0].resolve(inset.width()).coerceAtLeast(0f),
            clip.radiiVertical[0].resolve(inset.height()).coerceAtLeast(0f),
            clip.radiiHorizontal[1].resolve(inset.width()).coerceAtLeast(0f),
            clip.radiiVertical[1].resolve(inset.height()).coerceAtLeast(0f),
            clip.radiiHorizontal[2].resolve(inset.width()).coerceAtLeast(0f),
            clip.radiiVertical[2].resolve(inset.height()).coerceAtLeast(0f),
            clip.radiiHorizontal[3].resolve(inset.width()).coerceAtLeast(0f),
            clip.radiiVertical[3].resolve(inset.height()).coerceAtLeast(0f),
        ),
        inset.width(),
        inset.height(),
    )
    return Path().apply { addRoundRect(inset, radii, Path.Direction.CW) }
}
