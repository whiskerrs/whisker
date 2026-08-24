package rs.whisker.runtime

import android.content.Context
import android.graphics.Canvas
import android.graphics.Rect
import android.view.View
import android.view.ViewGroup
import android.widget.ScrollView
import kotlin.math.ceil
import kotlin.math.max

/**
 * Concrete ViewGroup used by Whisker containers.
 *
 * Rust owns every node's geometry. This class deliberately implements no
 * Android layout policy: it measures children from their assigned dimensions
 * and lays them out at the origin, preserving the x/y translation supplied by
 * the retained scene.
 */
public open class WhiskerContainerView(context: Context) : ViewGroup(context) {
    private var clipDescendantsHorizontally: Boolean = false
    private var clipDescendantsVertically: Boolean = false

    init {
        clipChildren = false
        clipToPadding = false
    }

    /** Applies protocol overflow clipping to descendants without clipping this View's background. */
    public fun setDescendantClip(horizontal: Boolean, vertical: Boolean) {
        if (
            clipDescendantsHorizontally == horizontal &&
            clipDescendantsVertically == vertical
        ) return
        clipDescendantsHorizontally = horizontal
        clipDescendantsVertically = vertical
        invalidate()
    }

    override fun dispatchDraw(canvas: Canvas) {
        if (!clipDescendantsHorizontally && !clipDescendantsVertically) {
            super.dispatchDraw(canvas)
            return
        }
        val visible = canvas.clipBounds
        val save = canvas.save()
        clipDescendants(
            canvas,
            horizontal = clipDescendantsHorizontally,
            vertical = clipDescendantsVertically,
            visible = visible,
        )
        super.dispatchDraw(canvas)
        canvas.restoreToCount(save)
    }

    /** Hook for the runtime wrapper to project rounded overflow geometry. */
    protected open fun clipDescendants(
        canvas: Canvas,
        horizontal: Boolean,
        vertical: Boolean,
        visible: Rect,
    ) {
        canvas.clipRect(
            if (horizontal) 0f else visible.left.toFloat(),
            if (vertical) 0f else visible.top.toFloat(),
            if (horizontal) width.toFloat() else visible.right.toFloat(),
            if (vertical) height.toFloat() else visible.bottom.toFloat(),
        )
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        var contentWidth = suggestedMinimumWidth
        var contentHeight = suggestedMinimumHeight
        for (index in 0 until childCount) {
            val child = getChildAt(index)
            if (child.visibility == View.GONE) continue
            val params = child.layoutParams
            val childWidth = if (params.width >= 0) {
                params.width
            } else {
                MeasureSpec.getSize(widthMeasureSpec)
            }
            val childHeight = if (params.height >= 0) {
                params.height
            } else {
                MeasureSpec.getSize(heightMeasureSpec)
            }
            child.measure(
                MeasureSpec.makeMeasureSpec(childWidth, MeasureSpec.EXACTLY),
                MeasureSpec.makeMeasureSpec(childHeight, MeasureSpec.EXACTLY),
            )
            contentWidth = max(contentWidth, ceil(child.x + child.measuredWidth).toInt())
            contentHeight = max(contentHeight, ceil(child.y + child.measuredHeight).toInt())
        }
        setMeasuredDimension(
            resolveSize(contentWidth, widthMeasureSpec),
            resolveSize(contentHeight, heightMeasureSpec),
        )
    }

    override fun onLayout(changed: Boolean, left: Int, top: Int, right: Int, bottom: Int) {
        for (index in 0 until childCount) {
            val child = getChildAt(index)
            if (child.visibility != View.GONE) {
                child.layout(0, 0, child.measuredWidth, child.measuredHeight)
            }
        }
    }
}

/** Native vertical scroll container with a dedicated multi-child content host. */
public class WhiskerScrollContainerView(context: Context) : ScrollView(context) {
    public val contentView: WhiskerContainerView = WhiskerContainerView(context)

    init {
        isFillViewport = true
        clipToPadding = false
        addView(
            contentView,
            LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT),
        )
    }
}
