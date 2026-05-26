// Compile-time smoke checks for the L-2a `ModuleDefinition` DSL.
//
// These samples exist purely so a `compileDebugKotlin` failure
// against the DSL surface surfaces here at build time. They're
// `internal` so they don't show up in the public module ABI;
// strip in a follow-up if they're holding back stripping the
// annotation-based API path.
//
// Phase L-2a — no runtime behavior; just exercises the type
// surface end-to-end.

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

        Constants("maxResolution" to "1080p")

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

    internal class StubModule : Module() {
        override fun definition(): ModuleDefinition = videoModuleDefinition()
    }
}
