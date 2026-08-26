package rs.whisker.runtime

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
                view.text = content.value
                view.textSize = content.fontSize
                view.setTextColor(content.color)
                view.setTypeface(view.typeface, if (content.fontWeight >= 600) 1 else 0)
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
