package rs.whisker.runtime

import android.graphics.Typeface
import android.os.Build
import android.util.TypedValue
import android.view.Gravity
import android.view.View

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
                view.setLineSpacing(0f, 1f)
                view.setWhiskerText(content)
                view.textDirection = when (content.direction) {
                    WhiskerTextDirection.AUTO -> View.TEXT_DIRECTION_FIRST_STRONG
                    WhiskerTextDirection.LEFT_TO_RIGHT -> View.TEXT_DIRECTION_LTR
                    WhiskerTextDirection.RIGHT_TO_LEFT -> View.TEXT_DIRECTION_RTL
                }
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
                // API 28+ can size lines from fallback fonts directly. Older
                // Android releases need font padding as the conservative
                // fallback, otherwise CJK glyph descenders can be clipped.
                includeFontPadding = Build.VERSION.SDK_INT < Build.VERSION_CODES.P
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
        View(WhiskerBuiltInElements.scrollView()) {
            Prop(
                "scroll-orientation",
                clear = { view: WhiskerScrollContainerView -> view.setScrollOrientation("vertical") },
            ) { view: WhiskerScrollContainerView, value ->
                view.setScrollOrientation(value.asString() ?: "vertical")
            }
            Prop(
                "item-snap",
                clear = WhiskerScrollContainerView::clearItemSnap,
            ) { view: WhiskerScrollContainerView, value ->
                val snap = (value as? WhiskerValue.Map)?.value.orEmpty()
                view.setItemSnap(
                    snap["factor"]?.asDouble() ?: 0.0,
                    snap["offset"]?.asDouble() ?: 0.0,
                )
            }
            Prop(
                "scroll-snap-stop",
                clear = { view: WhiskerScrollContainerView -> view.setScrollSnapStop("normal") },
            ) { view: WhiskerScrollContainerView, value ->
                view.setScrollSnapStop(value.asString() ?: "normal")
            }
            Prop(
                "enable-scroll",
                clear = { view: WhiskerScrollContainerView -> view.setUserScrollEnabled(true) },
            ) { view: WhiskerScrollContainerView, value ->
                view.setUserScrollEnabled(value.asBool() ?: true)
            }
            Command("scrollTo") { view: WhiskerScrollContainerView, value ->
                val arguments = (value as? WhiskerValue.Map)?.value.orEmpty()
                view.scrollToLogicalOffset(
                    arguments["offset"]?.asDouble() ?: 0.0,
                    arguments["smooth"]?.asBool() ?: false,
                )
            }
            Command("scrollBy") { view: WhiskerScrollContainerView, value ->
                val arguments = (value as? WhiskerValue.Map)?.value.orEmpty()
                view.scrollByLogicalOffset(
                    arguments["offset"]?.asDouble() ?: 0.0,
                    arguments["smooth"]?.asBool() ?: false,
                )
            }
            Events("scroll")
        }
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
