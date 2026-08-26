package rs.whisker.runtime.paint

import android.graphics.Path
import android.graphics.RectF

internal enum class HostClipReferenceBox { Border, Padding, Content }

internal sealed interface HostClipPath { val referenceBox: HostClipReferenceBox }

/** Rounded inset basic shape retained in physical Host coordinates. */
internal data class HostInsetClipPath(
    override val referenceBox: HostClipReferenceBox,
    val edges: List<HostPaintCoordinate>,
    val radiiHorizontal: List<HostPaintCoordinate>,
    val radiiVertical: List<HostPaintCoordinate>,
) : HostClipPath

internal data class HostCircleClipPath(
    override val referenceBox: HostClipReferenceBox,
    val radius: HostPaintCoordinate,
    val centerX: HostPaintCoordinate,
    val centerY: HostPaintCoordinate,
) : HostClipPath

internal data class HostEllipseClipPath(
    override val referenceBox: HostClipReferenceBox,
    val radiusX: HostPaintCoordinate,
    val radiusY: HostPaintCoordinate,
    val centerX: HostPaintCoordinate,
    val centerY: HostPaintCoordinate,
) : HostClipPath

internal data class HostPathCommand(
    val kind: Int,
    val points: List<HostPaintCoordinate>,
)

internal data class HostPathClipPath(
    override val referenceBox: HostClipReferenceBox,
    val evenOdd: Boolean,
    val commands: List<HostPathCommand>,
) : HostClipPath

internal fun resolveClipPath(
    clip: HostClipPath,
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
    if (clip is HostPathClipPath) {
        fun point(command: HostPathCommand, offset: Int) = android.graphics.PointF(
            reference.left + command.points[offset].resolve(reference.width()),
            reference.top + command.points[offset + 1].resolve(reference.height()),
        )
        return Path().apply {
            fillType = if (clip.evenOdd) Path.FillType.EVEN_ODD else Path.FillType.WINDING
            clip.commands.forEach { command ->
                when (command.kind) {
                    PATH_MOVE_TO -> point(command, 0).also { moveTo(it.x, it.y) }
                    PATH_LINE_TO -> point(command, 0).also { lineTo(it.x, it.y) }
                    PATH_QUADRATIC_TO -> {
                        val control = point(command, 0)
                        val end = point(command, 2)
                        quadTo(control.x, control.y, end.x, end.y)
                    }
                    PATH_CUBIC_TO -> {
                        val control1 = point(command, 0)
                        val control2 = point(command, 2)
                        val end = point(command, 4)
                        cubicTo(control1.x, control1.y, control2.x, control2.y, end.x, end.y)
                    }
                    PATH_CLOSE -> close()
                }
            }
        }
    }
    if (clip is HostCircleClipPath) {
        val centerX = reference.left + clip.centerX.resolve(reference.width())
        val centerY = reference.top + clip.centerY.resolve(reference.height())
        val diagonal = kotlin.math.hypot(reference.width(), reference.height()) / kotlin.math.sqrt(2f)
        val radius = clip.radius.resolve(diagonal).coerceAtLeast(0f)
        return Path().apply { addCircle(centerX, centerY, radius, Path.Direction.CW) }
    }
    if (clip is HostEllipseClipPath) {
        val centerX = reference.left + clip.centerX.resolve(reference.width())
        val centerY = reference.top + clip.centerY.resolve(reference.height())
        val radiusX = clip.radiusX.resolve(reference.width()).coerceAtLeast(0f)
        val radiusY = clip.radiusY.resolve(reference.height()).coerceAtLeast(0f)
        return Path().apply {
            addOval(RectF(centerX - radiusX, centerY - radiusY, centerX + radiusX, centerY + radiusY), Path.Direction.CW)
        }
    }
    clip as HostInsetClipPath
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

internal const val PATH_MOVE_TO = 0
internal const val PATH_LINE_TO = 1
internal const val PATH_QUADRATIC_TO = 2
internal const val PATH_CUBIC_TO = 3
internal const val PATH_CLOSE = 4
