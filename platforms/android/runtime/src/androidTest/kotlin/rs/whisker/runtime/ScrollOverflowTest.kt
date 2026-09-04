package rs.whisker.runtime

import android.view.View
import android.view.ViewGroup
import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ScrollOverflowTest {
    @Test
    fun horizontalContentIncludesVisibleDescendantOverflow() {
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            val context = ApplicationProvider.getApplicationContext<android.content.Context>()
            val scroll = WhiskerScrollContainerView(context)
            scroll.setScrollOrientation("horizontal")

            // Taffy can keep this auto-sized row at the 300px viewport width
            // while positioning its second card beyond that box.
            val row = WhiskerContainerView(context)
            val first = View(context)
            val second = View(context).apply { x = 240f }
            row.addView(first, ViewGroup.LayoutParams(200, 200))
            row.addView(second, ViewGroup.LayoutParams(200, 200))
            scroll.contentView.addView(row, ViewGroup.LayoutParams(300, 200))

            val exactWidth = View.MeasureSpec.makeMeasureSpec(300, View.MeasureSpec.EXACTLY)
            val exactHeight = View.MeasureSpec.makeMeasureSpec(200, View.MeasureSpec.EXACTLY)
            scroll.measure(exactWidth, exactHeight)
            scroll.layout(0, 0, 300, 200)

            assertEquals(440, scroll.contentView.measuredWidth)

            var scrollLeft = 0.0
            scroll.installWhiskerEventSink { event, detail ->
                if (event != "scroll") return@installWhiskerEventSink
                val values = (detail as WhiskerValue.Map).value
                scrollLeft = (values.getValue("scrollLeft") as WhiskerValue.Float).value
            }
            scroll.scrollTo(100, 0)
            assertTrue("overflowing cards must produce native horizontal range", scrollLeft > 0.0)
        }
    }
}
