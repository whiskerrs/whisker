package rs.whisker.elements.input

import android.view.inputmethod.InputConnection
import org.junit.Assert.assertTrue
import org.junit.Test

class UserEditInputConnectionTest {
    // `InputConnectionWrapper` forwards each overload straight to its target,
    // so an overload this class doesn't override bypasses the user-edit tag.
    private val textEditingMethods = setOf(
        "commitText",
        "setComposingText",
        "finishComposingText",
        "commitCorrection",
        "deleteSurroundingText",
        "deleteSurroundingTextInCodePoints",
        "sendKeyEvent",
        "replaceText",
        "performHandwritingGesture",
    )

    @Test
    fun overridesEveryTextEditingOverload() {
        val declared = UserEditInputConnection::class.java.declaredMethods
            .map { it.name to it.parameterTypes.toList() }
            .toSet()

        val missing = InputConnection::class.java.methods
            .filter { it.name in textEditingMethods }
            .map { it.name to it.parameterTypes.toList() }
            .filterNot { it in declared }

        assertTrue("not tagged as user edits: $missing", missing.isEmpty())
    }
}
