package rs.whisker.runtime

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.widget.HorizontalScrollView
import android.widget.ScrollView
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

/** Real traversal is required to exercise scrollToLogicalOffset's posted command. */
class ScrollCancellationActivity : Activity() {
    lateinit var scroll: WhiskerScrollContainerView
    var horizontal = false

    val nativeScroller: View
        get() = scroll.getChildAt(if (horizontal) 1 else 0)

    val offset: Int
        get() = if (horizontal) nativeScroller.scrollX else nativeScroller.scrollY

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        horizontal = intent.getBooleanExtra("horizontal", false)
        scroll = WhiskerScrollContainerView(this)
        scroll.setScrollOrientation(if (horizontal) "horizontal" else "vertical")
        scroll.contentView.addView(
            View(this),
            ViewGroup.LayoutParams(if (horizontal) 20_000 else 300, if (horizontal) 300 else 20_000),
        )
        setContentView(scroll, ViewGroup.LayoutParams(300, 300))
    }

    fun startMotion(smooth: Boolean) {
        if (horizontal) {
            val view = nativeScroller as HorizontalScrollView
            view.scrollTo(1_000, 0)
            if (smooth) view.smoothScrollTo(4_000, 0) else view.fling(8_000)
        } else {
            val view = nativeScroller as ScrollView
            view.scrollTo(0, 1_000)
            if (smooth) view.smoothScrollTo(0, 4_000) else view.fling(8_000)
        }
    }
}

@RunWith(AndroidJUnit4::class)
class ScrollCancellationTest {
    private fun verify(
        horizontal: Boolean,
        sameOffset: Boolean,
        smooth: Boolean = false,
        requestedOffset: Int? = null,
        nativeSnap: Boolean = false,
        delayFirstDraw: Boolean = false,
    ) {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val intent = Intent(instrumentation.context, ScrollCancellationActivity::class.java)
            .putExtra("horizontal", horizontal)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        ActivityScenario.launch<ScrollCancellationActivity>(intent).use { scenario ->
            instrumentation.waitForIdleSync()
            val completed = CountDownLatch(1)
            val samples = mutableListOf<Int>()
            var before = 0
            var target = 0
            scenario.onActivity { activity ->
                if (nativeSnap) activity.scroll.setItemSnap(0.0, 0.0)
                if (delayFirstDraw) {
                    // Model a busy CI renderer: timers run before the first
                    // frame that advances the native scroller.
                    val gate = ViewTreeObserver.OnPreDrawListener { false }
                    activity.scroll.viewTreeObserver.addOnPreDrawListener(gate)
                    activity.scroll.postDelayed({
                        activity.scroll.viewTreeObserver.removeOnPreDrawListener(gate)
                        activity.scroll.invalidate()
                    }, 160)
                }
                val interrupt = Runnable {
                    before = activity.offset
                    val requested = requestedOffset ?: if (sameOffset) before else 500
                    val maximum = if (horizontal) {
                        activity.scroll.contentView.width - activity.nativeScroller.width
                    } else {
                        activity.scroll.contentView.height - activity.nativeScroller.height
                    }
                    target = requested.coerceIn(0, maximum)
                    activity.scroll.scrollToLogicalOffset(
                        requested.toDouble() / activity.resources.displayMetrics.density,
                        false,
                    )
                    // Queued after the real command, then sample subsequent native frames.
                    activity.scroll.post {
                        samples.add(activity.offset)
                        val sample = object : Runnable {
                            override fun run() {
                                samples.add(activity.offset)
                                if (samples.size == 31) completed.countDown()
                                else activity.scroll.postOnAnimation(this)
                            }
                        }
                        activity.scroll.postOnAnimation(sample)
                    }
                }
                // A timer can run before any draw on a busy emulator. Wait for
                // actual native movement, then interrupt outside its callback
                // so the framework has finished updating its current frame.
                activity.scroll.installWhiskerPresentationSink { _, _ ->
                    if (activity.offset > 1_000) {
                        activity.scroll.installWhiskerPresentationSink(null)
                        activity.scroll.post(interrupt)
                    }
                }
                activity.startMotion(smooth)
            }
            assertTrue("native frames must complete", completed.await(10, TimeUnit.SECONDS))
            assertTrue("fixture must interrupt moving content: before=$before", before > 1_000)
            assertEquals("Instant must apply the target", target, samples.first())
            assertTrue(
                "old motion must not overwrite Instant: target=$target, samples=$samples",
                samples.all { it == target },
            )
            // A visually stationary but still-running scroller would steal the
            // next DOWN as an animation-interruption gesture instead of a tap.
            scenario.onActivity { activity ->
                val now = SystemClock.uptimeMillis()
                val down = MotionEvent.obtain(now, now, MotionEvent.ACTION_DOWN, 100f, 100f, 0)
                val cancel = MotionEvent.obtain(now, now, MotionEvent.ACTION_CANCEL, 100f, 100f, 0)
                try {
                    val native = activity.nativeScroller as ViewGroup
                    assertFalse("completed scroll must not intercept a new tap", native.onInterceptTouchEvent(down))
                    native.onInterceptTouchEvent(cancel)
                } finally {
                    down.recycle()
                    cancel.recycle()
                }
            }
        }
    }

    @Test fun horizontalFlingWaitsForDelayedFirstDraw() = verify(true, true, delayFirstDraw = true)
    @Test fun verticalFlingWaitsForDelayedFirstDraw() = verify(false, true, delayFirstDraw = true)
    @Test fun horizontalFlingStopsAtDifferentOffset() = verify(true, false)
    @Test fun horizontalFlingStopsAtSameOffset() = verify(true, true)
    @Test fun verticalFlingStopsAtDifferentOffset() = verify(false, false)
    @Test fun verticalFlingStopsAtSameOffset() = verify(false, true)
    @Test fun horizontalSmoothStopsAtSameOffset() = verify(true, true, true)
    @Test fun verticalSmoothStopsAtSameOffset() = verify(false, true, true)
    @Test fun horizontalFlingStopsAtClampedStart() = verify(true, false, requestedOffset = -1_000)
    @Test fun horizontalFlingStopsAtClampedEnd() = verify(true, false, requestedOffset = 30_000)
    @Test fun verticalFlingStopsAtClampedStart() = verify(false, false, requestedOffset = -1_000)
    @Test fun verticalFlingStopsAtClampedEnd() = verify(false, false, requestedOffset = 30_000)
    @Test fun horizontalInstantCancelsPendingSnap() = verify(true, false, nativeSnap = true)
    @Test fun verticalInstantCancelsPendingSnap() = verify(false, false, nativeSnap = true)
}
