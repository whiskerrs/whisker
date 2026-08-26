package rs.whisker.runtime

import android.graphics.Typeface
import android.os.Build
import android.util.TypedValue
import android.view.Gravity

/** Hand-written Android implementations matched to Rust registrations by name. */
public object WhiskerBuiltInElements {
    public const val VIEW: String = "whisker.ui/View"
    public const val TEXT: String = "whisker.ui/Text"
    public const val SCROLL_VIEW: String = "whisker.ui/ScrollView"

    @JvmStatic
    public fun view(): WhiskerElementFactory = WhiskerElementFactory(
        name = VIEW,
        makeView = ::WhiskerContainerView,
    )

    @JvmStatic
    public fun text(): WhiskerElementFactory =
        WhiskerElementFactory(
            name = TEXT,
            textUpdater = { view, content ->
                require(view is WhiskerTextView) { "$TEXT factory must create WhiskerTextView" }
                val density = view.resources.displayMetrics.density
                view.setTextSize(TypedValue.COMPLEX_UNIT_PX, content.fontSize * density)
                view.setTextColor(content.color)
                val baseTypeface = resolveWhiskerTypeface(content.fontFamilies)
                view.typeface = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    Typeface.create(
                        baseTypeface,
                        content.fontWeight.coerceIn(1, 1000),
                        content.fontStyle != WhiskerFontStyle.NORMAL,
                    )
                } else {
                    val style = (if (content.fontWeight >= 600) Typeface.BOLD else 0) or
                        (if (content.fontStyle != WhiskerFontStyle.NORMAL) Typeface.ITALIC else 0)
                    Typeface.create(baseTypeface, style)
                }
                view.letterSpacing = if (content.fontSize > 0f) {
                    content.letterSpacing / content.fontSize
                } else {
                    0f
                }
                content.lineHeight?.let { lineHeight ->
                    val fontHeight = view.paint.fontMetrics.run { descent - ascent }
                    view.setLineSpacing((lineHeight * density - fontHeight).coerceAtLeast(0f), 1f)
                } ?: view.setLineSpacing(0f, 1f)
                view.setWhiskerText(content)
                view.gravity = Gravity.TOP or when (content.alignment) {
                    WhiskerTextAlignment.START -> Gravity.START
                    WhiskerTextAlignment.END -> Gravity.END
                    WhiskerTextAlignment.LEFT -> Gravity.LEFT
                    WhiskerTextAlignment.RIGHT -> Gravity.RIGHT
                    WhiskerTextAlignment.CENTER -> Gravity.CENTER_HORIZONTAL
                }
                view.whiskerDecoration = content.decoration
                content.shadow?.let { shadow ->
                    val density = view.resources.displayMetrics.density
                    view.setShadowLayer(
                        shadow.blurRadius * density,
                        shadow.offsetX * density,
                        shadow.offsetY * density,
                        shadow.color,
                    )
                } ?: view.setShadowLayer(0f, 0f, 0f, 0)
            },
        ) { context ->
            WhiskerTextView(context).apply {
                includeFontPadding = false
                gravity = Gravity.TOP or Gravity.START
            }
        }

    @JvmStatic
    public fun scrollView(): WhiskerElementFactory =
        WhiskerElementFactory(
            name = SCROLL_VIEW,
            childrenHost = { view ->
                require(view is WhiskerScrollContainerView) {
                    "$SCROLL_VIEW factory must create WhiskerScrollContainerView"
                }
                view.contentView
            },
        ) { context ->
            WhiskerScrollContainerView(context)
        }
}

/** Built-ins use exactly the same checked-in ModuleDefinition path as libraries. */
@WhiskerModule
public class BuiltInElementModule : Module() {
    override fun definition(): ModuleDefinition = ModuleDefinition {
        Name("whisker.ui")
        View(WhiskerBuiltInElements.view())
        View(WhiskerBuiltInElements.text())
        View(WhiskerBuiltInElements.scrollView())
    }
}

/** Resolves Whisker's ordered CSS font-family fallback list for Android text Hosts. */
public fun resolveWhiskerTypeface(families: List<String>): Typeface {
    for (family in families) {
        if (family == "system") return Typeface.DEFAULT
        val candidate = Typeface.create(family, Typeface.NORMAL)
        if (candidate != Typeface.DEFAULT) return candidate
    }
    return Typeface.DEFAULT
}
