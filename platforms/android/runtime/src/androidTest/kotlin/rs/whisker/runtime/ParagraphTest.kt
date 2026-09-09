package rs.whisker.runtime

import android.graphics.Color
import android.text.SpannableString
import android.text.StaticLayout
import android.text.TextPaint
import android.text.style.MetricAffectingSpan
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ParagraphTest {
    @Test
    fun accessibilityActionsRetainIdentityOnlyForTheAcceptedContent() {
        InstrumentationRegistry.getInstrumentation().runOnMainSync {
            val fixture = fixture()
            val source = fixture.getString("text")
            val fields = fixture.getJSONObject("paragraph")
            fields.getJSONArray("runs").getJSONObject(0).put("action", 42)
            val paragraph = requireNotNull(WhiskerParagraph.decode(value(fields), source))
            val view = WhiskerTextView(InstrumentationRegistry.getInstrumentation().targetContext)
            val content = WhiskerTextContent(value = source, paragraph = paragraph, preparedContent = 7, fontSize = 16f, fontWeight = 400, color = Color.BLACK)
            val events = mutableListOf<WhiskerValue>()
            view.installWhiskerEventSink { name, detail -> if (name == "textactivate") events += detail }
            view.setWhiskerText(content)
            val info = android.view.accessibility.AccessibilityNodeInfo.obtain()
            view.onInitializeAccessibilityNodeInfo(info)
            val action = info.actionList.first { it.id >= 0x7f000000 }
            assertTrue(view.performAccessibilityAction(action.id, null))
            assertEquals(1, events.size)
            view.setWhiskerText(content.copy(preparedContent = 8))
            assertTrue(!view.performAccessibilityAction(action.id, null))
            view.setWhiskerText(content.copy(paragraph = paragraph.withVisibleEnd(0), preparedContent = 9))
            val hidden = android.view.accessibility.AccessibilityNodeInfo.obtain()
            view.onInitializeAccessibilityNodeInfo(hidden)
            assertTrue(hidden.actionList.none { it.id >= 0x7f000000 })
            assertEquals("", hidden.text.toString())
        }
    }

    @Test
    fun topAndBottomAttachmentsShareOneLineHeight() {
        val fixture = fixture("vertical-alignment")
        val source = fixture.getString("text")
        val paragraph = requireNotNull(WhiskerParagraph.decode(value(fixture.getJSONObject("paragraph")), source))
        val styled = SpannableString(source).apply { paragraph.apply(this, 1f) }
        val paint = TextPaint().apply { textSize = 16f }
        fun layout() = StaticLayout.Builder.obtain(styled, 0, styled.length, paint, 400).setIncludePad(false).build()
        paragraph.resolveVerticalAlignment(layout(), 1f)
        val final = layout()
        assertEquals(1, final.lineCount)
        assertTrue(final.height in 80..89)
        val placements = (paragraph.inlinePlacements(final, 1f) as WhiskerValue.Array).value
        fun y(index: Int) = ((placements[index] as WhiskerValue.Map).value["origin"] as WhiskerValue.Array).value[1].asDouble()!!
        assertEquals(y(0) + 80, y(1) + 50, 1.0)
    }

    @Test
    fun customTruncationReservesAnAtomicBoxOutsideTheSourceRange() {
        val fixture = fixture("truncation")
        val source = fixture.getString("text")
        val paragraph = requireNotNull(WhiskerParagraph.decode(value(fixture.getJSONObject("paragraph")), source))
        val paint = TextPaint().apply { textSize = 16f }
        for (width in listOf(120, 20)) {
            val (layout, clipped) = paragraph.truncateToFit(
                fits = { it.lineCount == 1 && it.getLineWidth(0) <= width },
                layout = { candidate ->
                    val styled = SpannableString(candidate.displayText()).apply { candidate.apply(this, 1f, width.toFloat()) }
                    StaticLayout.Builder.obtain(styled, 0, styled.length, paint, width).setIncludePad(false).build()
                },
            )
            assertEquals(1, layout.lineCount)
            assertTrue(layout.getLineWidth(0) <= width)
            val end = requireNotNull(clipped.visibleEnd)
            assertTrue(end < source.length)
            assertEquals(source.substring(0, end), clipped.copyText(0, layout.text.length))
            val geometry = (clipped.measurementGeometry(layout, 1f) as WhiskerValue.Map).value
            val lines = (geometry["lines"] as WhiskerValue.Array).value
            val line = (lines.single() as WhiskerValue.Map).value
            assertEquals(WhiskerValue.Int(source.length.toLong()), line["end"])
            assertEquals(WhiskerValue.Int((source.length - end).toLong()), line["ellipsis"])
        }
    }

    @Test
    fun mixedFontsUseTheSameSpansForMeasurementAndPaint() {
        val fixture = fixture()
        val text = fixture.getString("text")
        val paragraph = requireNotNull(WhiskerParagraph.decode(value(fixture.getJSONObject("paragraph")), text))
        val styled = SpannableString(text)
        paragraph.apply(styled, 1f)
        val run = styled.getSpans(9, 12, MetricAffectingSpan::class.java).single()
        val paint = TextPaint().apply { textSize = 16f }
        run.updateMeasureState(paint)
        assertEquals(36f, paint.textSize, 0f)
        run.updateDrawState(paint)
        assertEquals(36f, paint.textSize, 0f)
        assertEquals(Color.RED, paint.color)
        val base = TextPaint().apply { textSize = 16f }
        val narrow = StaticLayout.Builder.obtain(styled, 0, styled.length, base, 130).build()
        val wide = StaticLayout.Builder.obtain(styled, 0, styled.length, base, 600).build()
        assertTrue(narrow.height > wide.height)
        assertTrue(wide.getLineBaseline(0) > 20)
        assertNotNull(narrow.text)
    }

    @Test
    fun rejectsRangesInsideAnEmojiSurrogatePair() {
        val fixture = fixture()
        val paragraph = fixture.getJSONObject("paragraph")
        paragraph.getJSONArray("runs").getJSONObject(0).put("end", 7)
        assertThrows(IllegalArgumentException::class.java) {
            WhiskerParagraph.decode(value(paragraph), fixture.getString("text"))
        }
    }

    @Test
    fun inlineAttachmentWrapsAsAnAtomicMeasuredBox() {
        val fixture = fixture("attachment")
        val text = fixture.getString("text")
        val paragraph = requireNotNull(WhiskerParagraph.decode(value(fixture.getJSONObject("paragraph")), text))
        val styled = SpannableString(text)
        paragraph.apply(styled, 1f)
        val paint = TextPaint().apply { textSize = 16f }
        val wide = StaticLayout.Builder.obtain(styled, 0, styled.length, paint, 300).setIncludePad(false).build()
        val narrow = StaticLayout.Builder.obtain(styled, 0, styled.length, paint, 60).setIncludePad(false).build()
        assertTrue(wide.height >= 50)
        assertTrue(narrow.height > wide.height)
        val placement = ((paragraph.inlinePlacements(wide, 1f) as WhiskerValue.Array).value.single() as WhiskerValue.Map).value
        assertEquals(WhiskerValue.Int(22), placement["node"])
        val origin = (placement.getValue("origin") as WhiskerValue.Array).value
        assertEquals(wide.getLineBaseline(0).toDouble(), requireNotNull(origin[1].asDouble()) + 37.0, 0.5)
        val truncated = StaticLayout.Builder.obtain(styled, 0, styled.length, paint, 60)
            .setMaxLines(1).setEllipsize(android.text.TextUtils.TruncateAt.END).setEllipsizedWidth(60).build()
        val hidden = ((paragraph.inlinePlacements(truncated, 1f) as WhiskerValue.Array).value.single() as WhiskerValue.Map).value
        assertEquals(WhiskerValue.Null, hidden["origin"])
    }

    @Test
    fun roundedInlineBackgroundUsesFragmentGeometry() {
        val fixture = fixture("rounded-background")
        val text = fixture.getString("text")
        val paragraph = requireNotNull(WhiskerParagraph.decode(value(fixture.getJSONObject("paragraph")), text))
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        instrumentation.runOnMainSync {
            val view = WhiskerTextView(instrumentation.targetContext)
            view.setPadding(0, 0, 0, 0)
            view.includeFontPadding = false
            view.setWhiskerText(WhiskerTextContent(value = text, paragraph = paragraph, fontSize = 36f, fontWeight = 400, color = Color.TRANSPARENT))
            val width = 600
            val height = 180
            view.measure(android.view.View.MeasureSpec.makeMeasureSpec(width, android.view.View.MeasureSpec.EXACTLY), android.view.View.MeasureSpec.makeMeasureSpec(height, android.view.View.MeasureSpec.EXACTLY))
            view.layout(0, 0, width, height)
            val bitmap = android.graphics.Bitmap.createBitmap(width, height, android.graphics.Bitmap.Config.ARGB_8888)
            val canvas = android.graphics.Canvas(bitmap)
            canvas.drawColor(Color.WHITE)
            view.draw(canvas)
            fun isRed(x: Int, y: Int): Boolean = bitmap.getPixel(x, y).let { Color.red(it) > 200 && Color.green(it) < 30 && Color.blue(it) < 30 }
            val red = (0 until height).flatMap { y -> (0 until width).filter { x -> isRed(x, y) }.map { x -> x to y } }
            assertTrue(red.isNotEmpty())
            val left = red.minOf { it.first }; val right = red.maxOf { it.first }
            val top = red.minOf { it.second }; val bottom = red.maxOf { it.second }
            assertTrue(isRed((left + right) / 2, (top + bottom) / 2))
            assertTrue(!isRed(left + 1, top + 1))
            bitmap.recycle()
        }
    }

    private fun fixture(name: String = "styled"): JSONObject = JSONObject(
        InstrumentationRegistry.getInstrumentation().context.assets
            .open("paragraphs/$name.json").bufferedReader().use { it.readText() },
    )

    private fun value(raw: Any): WhiskerValue = when (raw) {
        JSONObject.NULL -> WhiskerValue.Null
        is JSONObject -> WhiskerValue.Map(raw.keys().asSequence().associateWith { value(raw.get(it)) })
        is JSONArray -> WhiskerValue.Array((0 until raw.length()).map { value(raw.get(it)) })
        else -> whiskerValueOf(raw)
    }
}
