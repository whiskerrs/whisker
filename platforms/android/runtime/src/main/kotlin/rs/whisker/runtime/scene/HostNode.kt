package rs.whisker.runtime.scene

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Matrix
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RectF
import android.os.Build
import android.view.MotionEvent
import android.view.PointerIcon
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import rs.whisker.runtime.WhiskerView
import rs.whisker.runtime.WhiskerContainerView
import rs.whisker.runtime.WhiskerMountedElement
import rs.whisker.runtime.paint.HostBoxPaint
import rs.whisker.runtime.paint.HostBackgroundLayers
import rs.whisker.runtime.paint.HostBoxShadow
import rs.whisker.runtime.paint.HostBackdropBlurRenderer
import rs.whisker.runtime.paint.HostClipPath
import rs.whisker.runtime.paint.HostImageRendering
import rs.whisker.runtime.paint.ResolvedBoxGeometry
import rs.whisker.runtime.paint.normalizeRadii
import rs.whisker.runtime.paint.drawInsetBoxShadows
import rs.whisker.runtime.paint.drawOuterBoxShadows

/** Mutable logical geometry attached to one Host scene node. */
internal data class HostGeometry(
    var x: Float = 0f,
    var y: Float = 0f,
    var width: Float = 0f,
    var height: Float = 0f,
    var contentX: Float = 0f,
    var contentY: Float = 0f,
    var contentWidth: Float = 0f,
    var contentHeight: Float = 0f,
)

internal data class HostAccessibility(
    val label: String?,
    val hint: String?,
    val role: String?,
    val identifier: String?,
    val hidden: Boolean,
    val modal: Boolean,
    val disabled: Boolean?,
    val selected: Boolean?,
    val checked: String?,
    val expanded: Boolean?,
)

/**
 * Common Android wrapper for every built-in or custom Whisker element.
 *
 * The scene owner controls hierarchy and geometry. Element modules only own
 * the mounted content View placed inside this wrapper.
 */
internal class HostNode(
    context: Context,
    val element: String,
    private val root: WhiskerView?,
) : WhiskerContainerView(context) {
    val geometry = HostGeometry()
    var paint: HostBoxPaint? = null
    var backgroundLayers: HostBackgroundLayers? = null
    var boxShadows: List<HostBoxShadow> = emptyList()
    var clipPath: HostClipPath? = null
    var mountedElement: WhiskerMountedElement? = null
    var zOrder: Int = 0
    var imageRendering: HostImageRendering = HostImageRendering.Auto
    var backdropBlur: Float = 0f
        set(value) {
            field = value
            invalidate()
        }
    internal var hitTestBehavior: Int = 0
        private set
    internal var cursorKeyword: Int = 0
        private set

    private val localTransform = Matrix()
    private var layoutTranslationX = 0f
    private var layoutTranslationY = 0f
    private var nativeTransformTranslationX = 0f
    private var nativeTransformTranslationY = 0f
    private var needsCanvasTransformFallback = false
    private var needsSoftwareCanvasTransform = false
    private var overflowClipRect = RectF()
    private var overflowClipPath: Path? = null
    private var paintClipPath: Path? = null
    private var resolvedBoxGeometry: ResolvedBoxGeometry? = null
    private var backdropRenderer: HostBackdropBlurRenderer? = null
    private var whiskerVisible = true

    init {
        setWillNotDraw(false)
    }

    fun setHitTestBehavior(value: Int) {
        require(value in 0..3)
        hitTestBehavior = value
    }

    fun setAccessibility(value: HostAccessibility) {
        val hasSemantics = value.label != null || value.hint != null || value.role != null ||
            value.disabled != null || value.selected != null || value.checked != null ||
            value.expanded != null
        contentDescription = value.label
        isEnabled = value.disabled != true
        isSelected = value.selected == true
        importantForAccessibility = if (value.hidden) {
            IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        } else if (hasSemantics) {
            IMPORTANT_FOR_ACCESSIBILITY_YES
        } else {
            IMPORTANT_FOR_ACCESSIBILITY_AUTO
        }
        if (Build.VERSION.SDK_INT >= 28) isScreenReaderFocusable = hasSemantics
        if (Build.VERSION.SDK_INT >= 28) {
            isAccessibilityHeading = value.role == "header"
            accessibilityPaneTitle = value.label.takeIf { value.modal }
        }
        accessibilityDelegate = object : View.AccessibilityDelegate() {
            override fun onInitializeAccessibilityNodeInfo(host: View, info: AccessibilityNodeInfo) {
                super.onInitializeAccessibilityNodeInfo(host, info)
                if (Build.VERSION.SDK_INT >= 18) info.viewIdResourceName = value.identifier
                info.className = when (value.role) {
                    "button" -> android.widget.Button::class.java.name
                    "image" -> android.widget.ImageView::class.java.name
                    "text", "header", "link" -> android.widget.TextView::class.java.name
                    "checkbox" -> android.widget.CheckBox::class.java.name
                    "radio" -> android.widget.RadioButton::class.java.name
                    "switch" -> android.widget.Switch::class.java.name
                    "adjustable" -> android.widget.SeekBar::class.java.name
                    "searchbox" -> android.widget.EditText::class.java.name
                    "tab" -> android.widget.Button::class.java.name
                    else -> android.view.ViewGroup::class.java.name
                }
                if (Build.VERSION.SDK_INT >= 26) info.hintText = value.hint
                value.checked?.let { checked ->
                    info.isCheckable = true
                    info.isChecked = checked == "true"
                }
                if (Build.VERSION.SDK_INT >= 30) {
                    info.stateDescription = when {
                        value.checked == "mixed" -> "mixed"
                        value.expanded == true -> "expanded"
                        value.expanded == false -> "collapsed"
                        else -> null
                    }
                }
            }
        }
    }

    fun setCursorKeyword(value: Int) {
        require(value in 0..34)
        cursorKeyword = value
        if (Build.VERSION.SDK_INT >= 24) {
            pointerIcon = if (value == 0) null else PointerIcon.getSystemIcon(
                context,
                when (value) {
                    1 -> PointerIcon.TYPE_ARROW
                    2 -> PointerIcon.TYPE_NULL
                    3 -> PointerIcon.TYPE_CONTEXT_MENU
                    4 -> PointerIcon.TYPE_HELP
                    5 -> PointerIcon.TYPE_HAND
                    6, 7 -> PointerIcon.TYPE_WAIT
                    8 -> PointerIcon.TYPE_CELL
                    9 -> PointerIcon.TYPE_CROSSHAIR
                    10 -> PointerIcon.TYPE_TEXT
                    11 -> PointerIcon.TYPE_VERTICAL_TEXT
                    12 -> PointerIcon.TYPE_ALIAS
                    13 -> PointerIcon.TYPE_COPY
                    14 -> PointerIcon.TYPE_ALL_SCROLL
                    15, 16 -> PointerIcon.TYPE_NO_DROP
                    17 -> PointerIcon.TYPE_GRAB
                    18 -> PointerIcon.TYPE_GRABBING
                    19, 22, 24, 29 -> PointerIcon.TYPE_HORIZONTAL_DOUBLE_ARROW
                    20, 21, 23, 30 -> PointerIcon.TYPE_VERTICAL_DOUBLE_ARROW
                    25, 28, 31 -> PointerIcon.TYPE_TOP_RIGHT_DIAGONAL_DOUBLE_ARROW
                    26, 27, 32 -> PointerIcon.TYPE_TOP_LEFT_DIAGONAL_DOUBLE_ARROW
                    33 -> PointerIcon.TYPE_ZOOM_IN
                    34 -> PointerIcon.TYPE_ZOOM_OUT
                    else -> PointerIcon.TYPE_ARROW
                },
            )
        }
    }

    /**
     * Sets this element's computed CSS visibility without hiding its View subtree.
     *
     * `View.INVISIBLE` would also suppress a descendant whose own computed
     * visibility is `visible`, which is not CSS visibility semantics.
     */
    fun setWhiskerVisibility(visible: Boolean) {
        if (whiskerVisible == visible) return
        whiskerVisible = visible
        mountedElement?.let { mounted ->
            if (mounted.childrenHost() == null) {
                mounted.view.visibility = if (visible) VISIBLE else INVISIBLE
            }
        }
        invalidate()
    }

    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (!whiskerVisible) return false
        return when (hitTestBehavior) {
            1 -> false
            2 -> onTouchEvent(event)
            else -> super.dispatchTouchEvent(event)
        }
    }

    override fun onTouchEvent(event: MotionEvent): Boolean =
        whiskerVisible && super.onTouchEvent(event)

    fun setOverflowClipGeometry(geometry: ResolvedBoxGeometry) {
        resolvedBoxGeometry = geometry
        val top = geometry.borderWidths[0].coerceIn(0f, geometry.height)
        val right = geometry.borderWidths[1].coerceIn(0f, geometry.width)
        val bottom = geometry.borderWidths[2].coerceIn(0f, geometry.height)
        val left = geometry.borderWidths[3].coerceIn(0f, geometry.width)
        overflowClipRect = RectF(left, top, geometry.width - right, geometry.height - bottom)
        if (overflowClipRect.isEmpty) {
            overflowClipPath = Path()
            invalidate()
            return
        }
        val outer = geometry.cornerRadii
        val inner = normalizeRadii(
            floatArrayOf(
                (outer[0] - left).coerceAtLeast(0f),
                (outer[1] - top).coerceAtLeast(0f),
                (outer[2] - right).coerceAtLeast(0f),
                (outer[3] - top).coerceAtLeast(0f),
                (outer[4] - right).coerceAtLeast(0f),
                (outer[5] - bottom).coerceAtLeast(0f),
                (outer[6] - left).coerceAtLeast(0f),
                (outer[7] - bottom).coerceAtLeast(0f),
            ),
            overflowClipRect.width(),
            overflowClipRect.height(),
        )
        overflowClipPath = Path().apply {
            addRoundRect(overflowClipRect, inner, Path.Direction.CW)
        }
        invalidate()
    }

    fun setPaintClipPath(path: Path?) {
        paintClipPath = path
        invalidate()
        (parent as? android.view.View)?.invalidate()
    }

    fun resolvedBorderWidths(): FloatArray = resolvedBoxGeometry?.borderWidths ?: FloatArray(4)

    fun setLayoutPosition(x: Float, y: Float) {
        layoutTranslationX = x
        layoutTranslationY = y
        translationX = x + nativeTransformTranslationX
        translationY = y + nativeTransformTranslationY
    }

    /** Applies a protocol transform around the local border-box origin. */
    @SuppressLint("NewApi")
    fun setLocalTransform(values: FloatArray, density: Float) {
        require(isProjectableFlatPlaneTransform(values))
        require(density.isFinite() && density > 0f)
        (parent as? android.view.View)?.invalidate()
        localTransform.setValues(
            floatArrayOf(
                values[0], values[4], values[12] * density,
                values[1], values[5], values[13] * density,
                values[3] / density, values[7] / density, values[15],
            ),
        )
        val isAxisAligned = values[1] == 0f && values[4] == 0f && values[3] == 0f &&
            values[7] == 0f && values[15] == 1f
        if (isAxisAligned) {
            clearNativeTransform()
            pivotX = 0f
            pivotY = 0f
            scaleX = values[0]
            scaleY = values[5]
            nativeTransformTranslationX = values[12] * density
            nativeTransformTranslationY = values[13] * density
            translationX = layoutTranslationX + nativeTransformTranslationX
            translationY = layoutTranslationY + nativeTransformTranslationY
            needsCanvasTransformFallback = false
            needsSoftwareCanvasTransform = false
        } else {
            pivotX = 0f
            pivotY = 0f
            scaleX = 1f
            scaleY = 1f
            nativeTransformTranslationX = 0f
            nativeTransformTranslationY = 0f
            translationX = layoutTranslationX
            translationY = layoutTranslationY
            needsCanvasTransformFallback = !applyNativeTransform(localTransform)
            needsSoftwareCanvasTransform = !needsCanvasTransformFallback
        }
        invalidate()
        (parent as? android.view.View)?.invalidate()
    }

    @SuppressLint("NewApi")
    private fun applyNativeTransform(matrix: Matrix): Boolean {
        if (!animationMatrixAvailable) return false
        return try {
            // setAnimationMatrix existed on RenderNode from API 21 and became public in API 29.
            // Unlike Canvas.concat, ViewGroup uses this matrix for native child hit testing too.
            setAnimationMatrix(matrix.takeUnless(Matrix::isIdentity))
            true
        } catch (_: NoSuchMethodError) {
            // An unusual OEM implementation may omit the formerly hidden method. Remember that
            // once so animated frames do not repeatedly pay for a linkage exception.
            animationMatrixAvailable = false
            false
        }
    }

    @SuppressLint("NewApi")
    private fun clearNativeTransform() {
        if (!animationMatrixAvailable) return
        try {
            setAnimationMatrix(null)
        } catch (_: NoSuchMethodError) {
            animationMatrixAvailable = false
        }
    }

    override fun draw(canvas: Canvas) {
        if (root?.shouldSkipBackdropCapture(this) == true) return
        if (
            !needsCanvasTransformFallback &&
            (!needsSoftwareCanvasTransform || canvas.isHardwareAccelerated)
        ) {
            drawClipped(canvas)
            return
        }
        val save = canvas.save()
        canvas.concat(localTransform)
        drawClipped(canvas)
        canvas.restoreToCount(save)
    }

    private fun drawClipped(canvas: Canvas) {
        val save = canvas.save()
        paintClipPath?.let(canvas::clipPath)
        if (whiskerVisible) {
            drawOuterBoxShadows(canvas, resolvedBoxGeometry, boxShadows)
            drawBackdropBlur(canvas)
            super.draw(canvas)
        } else {
            // View.draw() paints this node's background before dispatching
            // children. Dispatch only the child phase so a visible descendant
            // can override this node's hidden computed visibility.
            dispatchDraw(canvas)
        }
        canvas.restoreToCount(save)
    }

    private fun drawBackdropBlur(canvas: Canvas) {
        val captureRoot = root ?: return
        if (backdropBlur <= 0f || captureRoot.isRecordingBackdrop || Build.VERSION.SDK_INT < 31) {
            return
        }
        val geometry = resolvedBoxGeometry ?: return
        val clip = Path().apply {
            addRoundRect(
                RectF(0f, 0f, geometry.width, geometry.height),
                geometry.cornerRadii,
                Path.Direction.CW,
            )
        }
        val renderer = backdropRenderer ?: HostBackdropBlurRenderer().also {
            backdropRenderer = it
        }
        renderer.draw(canvas, captureRoot, this, backdropBlur, clip)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (whiskerVisible) {
            drawInsetBoxShadows(canvas, resolvedBoxGeometry, boxShadows)
        }
    }

    override fun clipDescendants(
        canvas: Canvas,
        horizontal: Boolean,
        vertical: Boolean,
        visible: Rect,
    ) {
        val path = overflowClipPath
        if (path == null) {
            super.clipDescendants(canvas, horizontal, vertical, visible)
            return
        }
        if (horizontal && vertical) {
            canvas.clipPath(path)
            return
        }
        canvas.clipRect(
            if (horizontal) overflowClipRect.left else visible.left.toFloat(),
            if (vertical) overflowClipRect.top else visible.top.toFloat(),
            if (horizontal) overflowClipRect.right else visible.right.toFloat(),
            if (vertical) overflowClipRect.bottom else visible.bottom.toFloat(),
        )
    }

    private companion object {
        private var animationMatrixAvailable = true
    }
}

/**
 * True when a finite 4x4 transform can be flattened onto this View's z=0 plane.
 *
 * Android Canvas accepts a 3x3 homography. For points `(x, y, 0, 1)`, only
 * rows x, y, and w of the 4x4 matrix contribute to projected screen
 * coordinates, so every finite Whisker matrix has an exact flat-plane
 * projection. Depth is intentionally flattened at each HostNode; preserving a
 * shared 3D descendant space is a separate capability.
 */
internal fun isProjectableFlatPlaneTransform(values: FloatArray): Boolean =
    values.size == 16 && values.all(Float::isFinite)
