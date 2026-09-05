package rs.whisker.runtime

import android.content.Context
import android.graphics.Canvas
import android.graphics.Rect
import android.view.MotionEvent
import android.view.View
import android.view.VelocityTracker
import android.view.ViewConfiguration
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.ScrollView
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.roundToInt

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
    internal var measuresDescendantOverflow: Boolean = false

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
            if (measuresDescendantOverflow) {
                val extent = descendantOverflowExtent(child)
                contentWidth = max(contentWidth, ceil(child.x + extent.right).toInt())
                contentHeight = max(contentHeight, ceil(child.y + extent.bottom).toInt())
            } else {
                contentWidth = max(contentWidth, ceil(child.x + child.measuredWidth).toInt())
                contentHeight = max(contentHeight, ceil(child.y + child.measuredHeight).toInt())
            }
        }
        setMeasuredDimension(
            resolveSize(contentWidth, widthMeasureSpec),
            resolveSize(contentHeight, heightMeasureSpec),
        )
    }

    /**
     * Returns the positive-axis visual extent of [view] in its own coordinate
     * space. A scroll container establishes a new overflow viewport, while an
     * ordinary Whisker View contributes descendants that visibly overflow it.
     */
    private fun descendantOverflowExtent(view: View): OverflowExtent {
        var right = view.measuredWidth.toFloat()
        var bottom = view.measuredHeight.toFloat()
        if (view !is ViewGroup || view is WhiskerScrollContainerView) {
            return OverflowExtent(right, bottom)
        }

        val clipsHorizontal = view is WhiskerContainerView && view.clipDescendantsHorizontally
        val clipsVertical = view is WhiskerContainerView && view.clipDescendantsVertically
        for (index in 0 until view.childCount) {
            val child = view.getChildAt(index)
            if (child.visibility == View.GONE) continue
            val childExtent = descendantOverflowExtent(child)
            if (!clipsHorizontal) right = max(right, child.x + childExtent.right)
            if (!clipsVertical) bottom = max(bottom, child.y + childExtent.bottom)
        }
        return OverflowExtent(right, bottom)
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

/** Native two-axis scroll container with a dedicated multi-child content host. */
public class WhiskerScrollContainerView(context: Context) : FrameLayout(context), WhiskerEventSource {
    public val contentView: WhiskerContainerView = WhiskerContainerView(context).apply {
        // Taffy may keep an auto-sized row at the viewport width while its
        // children visibly overflow it. Native ScrollView content size must
        // include that descendant overflow, matching CSS scrollable overflow.
        measuresDescendantOverflow = true
    }
    private var eventSink: ((String, WhiskerValue) -> Unit)? = null
    private var presentationSink: ((Float, Float) -> Unit)? = null
    private var horizontal = false
    private var snapFactor: Double? = null
    private var snapOffset = 0.0
    private var snapStopAlways = false
    private var userScrollEnabled = true
    private var scrollSequenceStart: Int? = null
    private var settleGeneration = 0
    private var dragging = false
    private var lastScrollX = 0.0
    private var lastScrollY = 0.0
    private var velocityTracker: VelocityTracker? = null
    private var activePointerId = MotionEvent.INVALID_POINTER_ID
    private var touchAction = -1

    private val verticalScroller = object : ScrollView(context) {
        fun scrollInstantlyTo(x: Int, y: Int) {
            super.scrollTo(x, y)
            // scrollTo clamps the position but leaves the framework scroller running.
            // A clamped overscroll finishes its springBack immediately when already
            // in bounds, cancelling both flings and smooth scrolls at this position.
            super.onOverScrolled(scrollX, scrollY, false, true)
        }

        override fun onInterceptTouchEvent(event: MotionEvent): Boolean {
            val intercepted = userScrollEnabled && super.onInterceptTouchEvent(event)
            if (intercepted) beginDrag()
            return intercepted
        }

        override fun onOverScrolled(x: Int, y: Int, clampedX: Boolean, clampedY: Boolean) {
            // The native scroller calls this only after its own touch slop and
            // nested-scroll arbitration have accepted movement, including edges.
            if (touchAction == MotionEvent.ACTION_MOVE) beginDrag()
            super.onOverScrolled(x, y, clampedX, clampedY)
        }

        override fun onTouchEvent(event: MotionEvent): Boolean {
            if (!userScrollEnabled) return false
            if (event.actionMasked == MotionEvent.ACTION_DOWN) {
                scrollSequenceStart = scrollY
            }
            val handled = super.onTouchEvent(event)
            if (event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_CANCEL) {
                scheduleSnap()
            }
            return handled
        }

        override fun fling(velocityY: Int) {
            if (snapStopAlways && snapFactor != null) {
                snapToAdjacentChild(velocityY.compareTo(0))
            } else {
                super.fling(velocityY)
                scheduleSnap()
            }
        }
    }

    private val horizontalScroller = object : HorizontalScrollView(context) {
        fun scrollInstantlyTo(x: Int, y: Int) {
            super.scrollTo(x, y)
            // Finish the framework scroller even when scrollTo did not change the
            // offset. Using the clamped position prevents a later spring-back frame
            // from overwriting the requested position.
            super.onOverScrolled(scrollX, scrollY, true, false)
        }

        override fun onInterceptTouchEvent(event: MotionEvent): Boolean {
            val intercepted = userScrollEnabled && super.onInterceptTouchEvent(event)
            if (intercepted) beginDrag()
            return intercepted
        }

        override fun onOverScrolled(x: Int, y: Int, clampedX: Boolean, clampedY: Boolean) {
            // The native scroller calls this only after its own touch slop and
            // nested-scroll arbitration have accepted movement, including edges.
            if (touchAction == MotionEvent.ACTION_MOVE) beginDrag()
            super.onOverScrolled(x, y, clampedX, clampedY)
        }

        override fun onTouchEvent(event: MotionEvent): Boolean {
            if (!userScrollEnabled) return false
            if (event.actionMasked == MotionEvent.ACTION_DOWN) {
                scrollSequenceStart = scrollX
            }
            val handled = super.onTouchEvent(event)
            if (event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_CANCEL) {
                scheduleSnap()
            }
            return handled
        }

        override fun fling(velocityX: Int) {
            if (snapStopAlways && snapFactor != null) {
                snapToAdjacentChild(velocityX.compareTo(0))
            } else {
                super.fling(velocityX)
                scheduleSnap()
            }
        }
    }

    init {
        clipToPadding = false
        verticalScroller.isFillViewport = true
        horizontalScroller.isFillViewport = true
        verticalScroller.clipToPadding = false
        horizontalScroller.clipToPadding = false
        addView(verticalScroller, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        addView(horizontalScroller, LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT))
        verticalScroller.addView(
            contentView,
            FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT),
        )
        horizontalScroller.visibility = View.GONE
        verticalScroller.setOnScrollChangeListener { _, _, _, _, _ -> emitScroll() }
        horizontalScroller.setOnScrollChangeListener { _, _, _, _, _ -> emitScroll() }
    }

    override fun installWhiskerEventSink(sink: ((String, WhiskerValue) -> Unit)?) {
        eventSink = sink
    }

    /** Installs the Host-internal scroll mirror, independent of app listeners. */
    public fun installWhiskerPresentationSink(sink: ((Float, Float) -> Unit)?) {
        presentationSink = sink
    }

    /** Switches the native scrolling axis without changing the Rust-owned child tree. */
    public fun setScrollOrientation(value: String) {
        val nextHorizontal = value == "horizontal"
        if (horizontal == nextHorizontal) return
        val old = activeScroller()
        val oldX = old.scrollX
        val oldY = old.scrollY
        (contentView.parent as? ViewGroup)?.removeView(contentView)
        horizontal = nextHorizontal
        if (horizontal) {
            horizontalScroller.addView(
                contentView,
                FrameLayout.LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.MATCH_PARENT),
            )
        } else {
            verticalScroller.addView(
                contentView,
                FrameLayout.LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT),
            )
        }
        verticalScroller.visibility = if (horizontal) View.GONE else View.VISIBLE
        horizontalScroller.visibility = if (horizontal) View.VISIBLE else View.GONE
        post { activeScroller().scrollTo(if (horizontal) oldX else 0, if (horizontal) 0 else oldY) }
    }

    /** Configures direct-child snapping in logical pixels. */
    public fun setItemSnap(factor: Double, offset: Double) {
        snapFactor = factor.coerceIn(0.0, 1.0)
        snapOffset = offset
    }

    public fun clearItemSnap() {
        snapFactor = null
        settleGeneration += 1
    }

    public fun setScrollSnapStop(value: String) {
        snapStopAlways = value == "always"
    }

    /** Enables touch-driven scrolling while keeping imperative commands live. */
    public fun setUserScrollEnabled(value: Boolean) {
        userScrollEnabled = value
        if (!value) {
            finishDrag(cancelled = true)
            verticalScroller.stopNestedScroll()
            horizontalScroller.stopNestedScroll()
        }
    }

    public fun scrollToLogicalOffset(offset: Double, smooth: Boolean) {
        val pixels = (offset * resources.displayMetrics.density).roundToInt()
        val isHorizontal = horizontal
        val apply = {
            if (!smooth) {
                // A settle check from the interrupted gesture must not start
                // another animation after the explicit position has been applied.
                settleGeneration += 1
                scrollSequenceStart = null
            }
            if (isHorizontal) {
                if (smooth) horizontalScroller.smoothScrollTo(pixels, 0)
                else horizontalScroller.scrollInstantlyTo(pixels, 0)
            } else {
                if (smooth) verticalScroller.smoothScrollTo(0, pixels)
                else verticalScroller.scrollInstantlyTo(0, pixels)
            }
        }
        // A FramePacket can resize the scroll content and issue scrollTo in
        // the same Host turn. Applying synchronously lets the following
        // Android layout pass clamp the offset against the previous extent
        // and reset it to zero. Defer every command until all operations in
        // the packet have been staged and the pending layout has completed.
        post { apply() }
    }

    public fun scrollByLogicalOffset(offset: Double, smooth: Boolean) {
        val pixels = (offset * resources.displayMetrics.density).roundToInt()
        if (horizontal) {
            if (smooth) horizontalScroller.smoothScrollBy(pixels, 0)
            else horizontalScroller.scrollBy(pixels, 0)
        } else {
            if (smooth) verticalScroller.smoothScrollBy(0, pixels)
            else verticalScroller.scrollBy(0, pixels)
        }
    }

    override fun scrollTo(x: Int, y: Int) {
        if (horizontal) {
            horizontalScroller.scrollTo(x, 0)
        } else {
            verticalScroller.scrollTo(0, y)
        }
    }

    override fun dispatchTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            finishDrag(cancelled = true)
            velocityTracker = VelocityTracker.obtain()
            activePointerId = event.getPointerId(0)
        }
        velocityTracker?.addMovement(event)
        if (event.actionMasked == MotionEvent.ACTION_POINTER_DOWN) {
            activePointerId = event.getPointerId(event.actionIndex)
        } else if (event.actionMasked == MotionEvent.ACTION_POINTER_UP &&
            event.getPointerId(event.actionIndex) == activePointerId
        ) {
            activePointerId = event.getPointerId(if (event.actionIndex == 0) 1 else 0)
            velocityTracker?.clear()
        }
        touchAction = event.actionMasked
        val handled = try {
            super.dispatchTouchEvent(event)
        } finally {
            touchAction = -1
        }
        // Emit after native UP processing, so an app's Instant command cancels
        // the fling that native handling just started, rather than preceding it.
        if (event.actionMasked == MotionEvent.ACTION_UP || event.actionMasked == MotionEvent.ACTION_CANCEL) {
            finishDrag(cancelled = event.actionMasked == MotionEvent.ACTION_CANCEL)
        }
        return handled
    }

    private fun beginDrag() {
        if (dragging || !userScrollEnabled || velocityTracker == null) return
        dragging = true
        scrollSequenceStart = if (horizontal) horizontalScroller.scrollX else verticalScroller.scrollY
        emitScroll()
    }

    private fun finishDrag(cancelled: Boolean) {
        var velocity = 0.0
        if (dragging && !cancelled) {
            velocityTracker?.let { tracker ->
                tracker.computeCurrentVelocity(1000, ViewConfiguration.get(context).scaledMaximumFlingVelocity.toFloat())
                // Pointer motion and content offset have opposite signs.
                velocity = -(if (horizontal) tracker.getXVelocity(activePointerId)
                    else tracker.getYVelocity(activePointerId)) / resources.displayMetrics.density.toDouble()
            }
        }
        velocityTracker?.recycle()
        velocityTracker = null
        activePointerId = MotionEvent.INVALID_POINTER_ID
        if (!dragging) return
        dragging = false
        emitScroll(velocity, cancelled)
    }

    override fun onDetachedFromWindow() {
        finishDrag(cancelled = true)
        super.onDetachedFromWindow()
    }

    private fun activeScroller(): View = if (horizontal) horizontalScroller else verticalScroller

    private fun emitScroll(releaseVelocity: Double = 0.0, cancelled: Boolean = false) {
        val density = resources.displayMetrics.density.toDouble()
        val scroller = activeScroller()
        val x = scroller.scrollX / density
        val y = scroller.scrollY / density
        val dx = x - lastScrollX
        val dy = y - lastScrollY
        lastScrollX = x
        lastScrollY = y
        presentationSink?.invoke(
            (scroller.scrollX / density).toFloat(),
            (scroller.scrollY / density).toFloat(),
        )
        eventSink?.invoke(
            "scroll",
            WhiskerValue.Map(
                mapOf(
                    "deltaX" to WhiskerValue.Float(dx),
                    "deltaY" to WhiskerValue.Float(dy),
                    "isDragging" to WhiskerValue.Bool(dragging),
                    "velocityX" to WhiskerValue.Float(if (horizontal) releaseVelocity else 0.0),
                    "velocityY" to WhiskerValue.Float(if (horizontal) 0.0 else releaseVelocity),
                    "isDragCancelled" to WhiskerValue.Bool(cancelled),
                    "scrollLeft" to WhiskerValue.Float(scroller.scrollX / density),
                    "scrollTop" to WhiskerValue.Float(scroller.scrollY / density),
                    "scrollWidth" to WhiskerValue.Float(contentView.width / density),
                    "scrollHeight" to WhiskerValue.Float(contentView.height / density),
                    "viewportWidth" to WhiskerValue.Float(width / density),
                    "viewportHeight" to WhiskerValue.Float(height / density),
                ),
            ),
        )
    }

    private fun scheduleSnap() {
        if (snapFactor == null) return
        val generation = ++settleGeneration
        var previous = if (horizontal) horizontalScroller.scrollX else verticalScroller.scrollY
        var stableFrames = 0
        val check = object : Runnable {
            override fun run() {
                if (generation != settleGeneration || snapFactor == null) return
                val current = if (horizontal) horizontalScroller.scrollX else verticalScroller.scrollY
                if (current == previous) stableFrames += 1 else stableFrames = 0
                previous = current
                if (stableFrames >= 2) {
                    snapToNearestChild()
                } else {
                    postDelayed(this, 32L)
                }
            }
        }
        postDelayed(check, 32L)
    }

    private fun snapToNearestChild() {
        val current = if (horizontal) horizontalScroller.scrollX else verticalScroller.scrollY
        val start = scrollSequenceStart ?: current
        val targets = snapTargets()
        val target = (if (snapStopAlways && current > start) {
            targets.firstOrNull { it > start } ?: targets.lastOrNull()
        } else if (snapStopAlways && current < start) {
            targets.lastOrNull { it < start } ?: targets.firstOrNull()
        } else {
            targets.minByOrNull { kotlin.math.abs(it - current) }
        }) ?: return
        scrollSequenceStart = null
        if (target == current) return
        if (horizontal) horizontalScroller.smoothScrollTo(target, 0)
        else verticalScroller.smoothScrollTo(0, target)
    }

    private fun snapToAdjacentChild(direction: Int) {
        val current = if (horizontal) horizontalScroller.scrollX else verticalScroller.scrollY
        val start = scrollSequenceStart ?: current
        val targets = snapTargets()
        val target = when {
            direction > 0 -> targets.firstOrNull { it > start } ?: targets.lastOrNull()
            direction < 0 -> targets.lastOrNull { it < start } ?: targets.firstOrNull()
            else -> targets.minByOrNull { kotlin.math.abs(it - current) }
        } ?: return
        scrollSequenceStart = null
        settleGeneration += 1
        if (horizontal) horizontalScroller.smoothScrollTo(target, 0)
        else verticalScroller.smoothScrollTo(0, target)
    }

    private fun snapTargets(): List<Int> {
        val factor = snapFactor ?: return emptyList()
        if (contentView.childCount == 0) return emptyList()
        val density = resources.displayMetrics.density.toDouble()
        val viewport = if (horizontal) width else height
        val contentExtent = if (horizontal) contentView.width else contentView.height
        val maxOffset = (contentExtent - viewport).coerceAtLeast(0)
        return (0 until contentView.childCount)
            .map { contentView.getChildAt(it) }
            .map { child ->
                val start = if (horizontal) child.x.toDouble() else child.y.toDouble()
                val size = if (horizontal) child.width.toDouble() else child.height.toDouble()
                (start + size * factor - viewport * factor + snapOffset * density)
                    .toInt()
                    .coerceIn(0, maxOffset)
            }
            .distinct()
            .sorted()
    }
}

private data class OverflowExtent(val right: Float, val bottom: Float)
