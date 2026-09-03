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
        fun setFontSize(value: Float) { /* noop */ }
    }

    internal fun videoModuleDefinition(): ModuleDefinition = ModuleDefinition {
        Name("Video")

        View("sample-module:Video", FakeVideoView::class.java) {
            Prop("src") { view: FakeVideoView, value ->
                view.setSrc(value.asString() ?: "")
            }
            Command("play") { view: FakeVideoView, _ -> view.play() }
            Command("pause") { view: FakeVideoView, _ -> view.pause() }
            Command("seek") { view: FakeVideoView, parameters ->
                view.seek(parameters.asDouble() ?: 0.0)
            }
            Events("onCompleted")
            TextStyle { view: FakeVideoView, style -> view.setFontSize(style.fontSize) }
            Measurement { request ->
                WhiskerMeasuredSize(request.knownWidth ?: 160f, request.knownHeight ?: 90f)
            }
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
