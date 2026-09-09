package rs.whisker.runtime

import android.graphics.Color
import android.graphics.Typeface
import android.os.Build
import android.text.Spannable
import android.text.Spanned
import android.text.TextPaint
import android.text.style.MetricAffectingSpan

public class WhiskerParagraph private constructor(internal val runs: List<ParagraphRun>, public val attachments: List<WhiskerInlineAttachment>, public val source: String, public val visibleEnd: Int? = null) {
    public fun withVisibleEnd(end: Int): WhiskerParagraph = WhiskerParagraph(runs.mapNotNull { run -> if (run.start < end) run.copy(end = minOf(run.end, end)) else null }, attachments, source, end)
    public fun displayText(): String = if (visibleEnd != null && attachments.any { it.truncation }) source.substring(0, visibleEnd) + "\uFFFC" else source
    public fun copyText(start: Int, end: Int): String {
        val limit = if (attachments.any { it.truncation }) visibleEnd ?: source.length else source.length
        val first = start.coerceIn(0, limit)
        val last = end.coerceIn(first, limit)
        val result = StringBuilder()
        var cursor = first
        attachments.filter { !it.truncation && it.start >= first && it.end <= last }.forEach { attachment ->
            result.append(source, cursor, attachment.start)
            result.append(attachment.label ?: "")
            cursor = attachment.end
        }
        return result.append(source, cursor, last).toString()
    }

    public val accessibleText: String get() = source.substring(0, visibleEnd ?: source.length).replace("\uFFFC", "")
    internal val accessibleActions: List<Pair<Long, String>> get() {
        val actions = linkedMapOf<Long, String>()
        runs.forEach { run ->
            val action = run.action ?: return@forEach
            val end = minOf(run.end, visibleEnd ?: source.length)
            if (run.start >= end) return@forEach
            val label = source.substring(run.start, end).replace("\uFFFC", "")
            if (label.isNotEmpty()) actions[action] = (actions[action] ?: "") + label
        }
        return actions.toList()
    }

    public val sourceRanges: List<IntRange> get() = runs.map { it.start until it.end }

    public fun apply(text: Spannable, density: Float, width: Float = Float.POSITIVE_INFINITY) {
        runs.forEach { run ->
            text.setSpan(ParagraphSpan(run, density), run.start, run.end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
        attachments.forEach { attachment ->
            val placed = if (attachment.truncation) {
                if (visibleEnd == null) return@forEach
                attachment.copy(start = visibleEnd, end = visibleEnd + 1, width = minOf(attachment.width, width))
            } else attachment
            if (placed.end <= text.length) text.setSpan(WhiskerInlineSpan(placed, density), placed.start, placed.end, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        }
    }

    public companion object {
        public fun decode(value: WhiskerValue?, text: String): WhiskerParagraph? {
            if (value == null || value == WhiskerValue.Null) return null
            val map = value.fields()
            require(map.integer("version") == 1)
            var previousEnd = 0
            val runs = map.getValue("runs").items().map { raw ->
                val fields = raw.fields()
                val start = fields.integer("start")
                val end = fields.integer("end")
                require(start >= previousEnd && end > start && end <= text.length)
                require(start == 0 || !Character.isLowSurrogate(text[start]))
                require(end == text.length || !Character.isLowSurrogate(text[end]))
                previousEnd = end
                val families = fields.getValue("families").items().map { requireNotNull(it.asString()) }
                require(families.isNotEmpty() && families.all { it.isNotEmpty() })
                val size = fields.number("size")
                val weight = fields.integer("weight")
                require(size > 0f && weight in 1..1000)
                val variations = fields.getValue("variations").fields().mapValues { requireNotNull(it.value.asDouble()) }.toMutableMap()
                if (fields.getValue("optical").asBool() == true && "opsz" !in variations) {
                    variations["opsz"] = size.toDouble()
                }
                ParagraphRun(
                    start, end, families, size, weight,
                    requireNotNull(fields.getValue("italic").asBool()),
                    fields.number("spacing"),
                    fields.getValue("features").fields().entries.joinToString(", ") { "'${it.key}' ${requireNotNull(it.value.asInt())}" }.ifEmpty { null },
                    variations.entries.joinToString(", ") { "'${it.key}' ${it.value}" }.ifEmpty { null },
                    fields["paint"]?.takeUnless { it == WhiskerValue.Null }?.let(::decodePaint),
                    fields["alignment"]?.asInt()?.toInt()?.also { require(it in 0..4) } ?: 0,
                    fields["shift"]?.asDouble()?.toFloat()?.also { require(it.isFinite()) } ?: 0f,
                    fields["action"]?.asInt()?.takeIf { it > 0 },
                )
            }
            val attachments = (map["attachments"] ?: WhiskerValue.Array(emptyList())).items().map { raw ->
                val fields = raw.fields()
                val start = fields.integer("start")
                val end = fields.integer("end")
                val width = fields.number("width")
                val height = fields.number("height")
                val alignment = fields.integer("alignment")
                val node = requireNotNull(fields.getValue("node").asInt())
                val truncation = fields["truncation"]?.asBool() == true
                require(node > 0 && start >= 0 && if (truncation) start == text.length && end == start else start < text.length && end == start + 1 && text[start] == '\uFFFC')
                require(width >= 0f && height >= 0f && alignment in 0..4)
                WhiskerInlineAttachment(node, start, end, width, height, fields.number("baseline"), alignment, fields.number("shift"), truncation, fields["label"]?.asString())
            }
            require(attachments.map { it.node }.distinct().size == attachments.size)
            require(attachments.map { it.start }.distinct().size == attachments.size)
            val full = WhiskerParagraph(runs, attachments, text)
            val end = map["visibleEnd"]?.asInt()?.toInt()?.also { require(it in 0..text.length) }
            return if (end != null && attachments.any { it.truncation }) full.withVisibleEnd(end) else full
        }
    }
}

internal data class ParagraphRun(
    val start: Int,
    val end: Int,
    val families: List<String>,
    val size: Float,
    val weight: Int,
    val italic: Boolean,
    val spacing: Float,
    val features: String?,
    val variations: String?,
    val paint: ParagraphPaint?,
    val alignment: Int = 0,
    val shift: Float = 0f,
    val action: Long? = null,
)

internal data class ParagraphPaint(
    val color: Int,
    val background: Int,
    val underline: Boolean,
    val strike: Boolean,
    val shadow: ParagraphShadow?,
    val radii: List<FloatArray>,
    val decorationColor: Int,
    val decorationStyle: WhiskerTextDecorationStyle,
)

internal data class ParagraphShadow(val x: Float, val y: Float, val blur: Float, val color: Int)

internal class ParagraphSpan(private val run: ParagraphRun, private val density: Float) : MetricAffectingSpan() {
    var paintStyle: ParagraphPaint? = run.paint
    private val font: Typeface = resolveWhiskerTypeface(run.families).let { base ->
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) Typeface.create(base, run.weight, run.italic)
        else Typeface.create(base, (if (run.weight >= 600) Typeface.BOLD else 0) or (if (run.italic) Typeface.ITALIC else 0))
    }

    override fun updateMeasureState(paint: TextPaint) {
        paint.typeface = font
        paint.textSize = run.size * density
        paint.letterSpacing = run.spacing / run.size
        paint.baselineShift = if (run.alignment == 4) -(run.shift * density).toInt() else 0
        paint.fontFeatureSettings = run.features
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) paint.fontVariationSettings = run.variations
    }

    override fun updateDrawState(paint: TextPaint) {
        updateMeasureState(paint)
        paintStyle?.let { style ->
            paint.color = style.color
            paint.bgColor = Color.TRANSPARENT
            paint.isUnderlineText = false
            paint.isStrikeThruText = false
            paint.clearShadowLayer()
            style.shadow?.let { paint.setShadowLayer(it.blur * density, it.x * density, it.y * density, it.color) }
        }
    }
}

private fun decodePaint(value: WhiskerValue): ParagraphPaint {
    val fields = value.fields()
    return ParagraphPaint(
        decodeColor(fields.getValue("color")),
        fields["background"]?.takeUnless { it == WhiskerValue.Null }?.let(::decodeColor) ?: Color.TRANSPARENT,
        requireNotNull(fields.getValue("underline").asBool()),
        requireNotNull(fields.getValue("strike").asBool()),
        fields.getValue("shadows").items().firstOrNull()?.fields()?.let {
            ParagraphShadow(it.number("x"), it.number("y"), it.number("blur"), decodeColor(it.getValue("color")))
        },
        (fields["radii"] ?: WhiskerValue.Array(List(4) { WhiskerValue.Array(List(4) { WhiskerValue.Float(0.0) }) })).items().map { raw ->
            raw.items().map { requireNotNull(it.asDouble()).toFloat().also { value -> require(value.isFinite() && value >= 0) } }.toFloatArray().also { require(it.size == 4) }
        }.also { require(it.size == 4) },
        decodeColor(fields.getValue("decorationColor")),
        WhiskerTextDecorationStyle.entries[fields.integer("decorationStyle").also { require(it in 0..4) }],
    )
}

private fun decodeColor(value: WhiskerValue): Int {
    value.asString()?.let { return if (it == "transparent") Color.TRANSPARENT else Color.parseColor(it) }
    val channels = value.items().map { requireNotNull(it.asDouble()).toFloat() }
    require(channels.size == 4 && channels.all { it.isFinite() })
    return Color.argb((channels[3] * 255).toInt(), channels[0].toInt(), channels[1].toInt(), channels[2].toInt())
}

private fun WhiskerValue.fields(): Map<String, WhiskerValue> = (this as? WhiskerValue.Map)?.value
    ?: error("expected paragraph object")
private fun WhiskerValue.items(): List<WhiskerValue> = (this as? WhiskerValue.Array)?.value
    ?: error("expected paragraph array")
private fun Map<String, WhiskerValue>.integer(name: String): Int = requireNotNull(getValue(name).asInt()).let {
    require(it in 0..Int.MAX_VALUE.toLong())
    it.toInt()
}
private fun Map<String, WhiskerValue>.number(name: String): Float = requireNotNull(getValue(name).asDouble()).toFloat().also {
    require(it.isFinite())
}
