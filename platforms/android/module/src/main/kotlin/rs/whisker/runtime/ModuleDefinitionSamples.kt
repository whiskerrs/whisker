// Compile-time smoke checks for the `ModuleDefinition` DSL.
//
// These samples exist purely so a break in the DSL surface fails
// `compileDebugKotlin` here. `internal` so they stay out of the
// public module ABI. No runtime behavior — they just exercise the
// type surface end-to-end.

package rs.whisker.runtime

@Suppress("unused", "TestFunctionName")
internal object ModuleDefinitionSamples {

    // ---- View-bearing module --------------------------------------------

    internal class FakeVideoView {
        fun setSrc(value: String) { /* noop */ }
        fun play() { /* noop */ }
        fun pause() { /* noop */ }
        fun seek(seconds: Double) { /* noop */ }
    }

    internal fun videoModuleDefinition(): ModuleDefinition = ModuleDefinition {
        Name("Video")

        Constants("maxResolution" to WhiskerValue.Str("1080p"))

        View(FakeVideoView::class.java) {
            Prop("src") { view: FakeVideoView, value ->
                view.setSrc(value.asString() ?: "")
            }
            Function("play") { view: FakeVideoView, _ -> view.play(); WhiskerValue.Null }
            Function("pause") { view: FakeVideoView, _ -> view.pause(); WhiskerValue.Null }
            Function("seek") { view: FakeVideoView, args ->
                view.seek(args.getOrNull(0)?.asDouble() ?: 0.0)
                WhiskerValue.Null
            }
            Events("onCompleted")
        }
    }

    // ---- Function-only (view-less) module ------------------------------

    internal fun localStoreModuleDefinition(): ModuleDefinition = ModuleDefinition {
        Name("WhiskerLocalStore")

        Function("save") { args ->
            val key = args.getOrNull(0)?.asString() ?: ""
            val value = args.getOrNull(1)?.asString() ?: ""
            WhiskerValue.Bool(key.isNotEmpty() && value.isNotEmpty())
        }
        Function("load") { args ->
            WhiskerValue.Str("stub-value-for-${args.getOrNull(0)?.asString() ?: ""}")
        }
    }

    // ---- Module subclass shape -----------------------------------------

    @WhiskerModule
    internal class StubModule : Module() {
        override fun definition(): ModuleDefinition = videoModuleDefinition()
    }
}
