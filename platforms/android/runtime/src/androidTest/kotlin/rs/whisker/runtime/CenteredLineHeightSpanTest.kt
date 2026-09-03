package rs.whisker.runtime

import android.text.SpannableString
import android.text.Spanned
import android.text.StaticLayout
import android.text.TextPaint
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.SdkSuppress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import rs.whisker.runtime.internal.CenteredLineHeightSpan
import rs.whisker.runtime.internal.centeredLineAscent
import rs.whisker.runtime.measure.configureFallbackLineSpacing

@RunWith(AndroidJUnit4::class)
class CenteredLineHeightSpanTest {
    @Test
    @SdkSuppress(minSdkVersion = 28)
    fun textRenderingAndMeasurementIncludeFallbackFontMetrics() {
        val context = androidx.test.core.app.ApplicationProvider
            .getApplicationContext<android.content.Context>()
        val view = WhiskerTextView(context)
        assertTrue(view.isFallbackLineSpacing)

        val paint = TextPaint().apply { textSize = 32f }
        val text = "日本語"
        val layout = configureFallbackLineSpacing(
            StaticLayout.Builder.obtain(text, 0, text.length, paint, 1024),
        ).build()
        assertTrue(layout.isFallbackLineSpacingEnabled)
    }

    @Test
    fun staticLayoutUsesTheCenteredBaselineAndRequestedLineHeight() {
        val paint = TextPaint().apply { textSize = 32f }
        val targetHeight = 64
        val value = SpannableString("Whisker").apply {
            setSpan(
                CenteredLineHeightSpan(targetHeight.toFloat()),
                0,
                length,
                Spanned.SPAN_INCLUSIVE_EXCLUSIVE,
            )
        }
        val natural = paint.fontMetricsInt
        val expectedBaseline = -centeredLineAscent(
            natural.ascent,
            natural.descent,
            targetHeight,
        )
        val layout = StaticLayout.Builder.obtain(value, 0, value.length, paint, 1024)
            .setIncludePad(false)
            .build()

        assertEquals(targetHeight, layout.height)
        assertEquals(expectedBaseline, layout.getLineBaseline(0))
    }
}
