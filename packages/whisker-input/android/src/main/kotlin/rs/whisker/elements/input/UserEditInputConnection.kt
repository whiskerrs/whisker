package rs.whisker.elements.input

import android.view.KeyEvent
import android.view.inputmethod.CorrectionInfo
import android.view.inputmethod.HandwritingGesture
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputConnectionWrapper
import android.view.inputmethod.TextAttribute
import java.util.concurrent.Executor
import java.util.function.IntConsumer

/// `InputConnection` that reports every text edit an IME makes through
/// [onUserEdit] before forwarding it to [target].
///
/// `WhiskerInputView` only writes an edit back to the bound signal when
/// it was tagged here, so an editing entry point missing from this class
/// silently drops the user's text. `InputConnectionWrapper` forwards each
/// overload straight to its target rather than through the older
/// signature, so every overload needs its own override.
internal class UserEditInputConnection(
    target: InputConnection,
    private val onUserEdit: () -> Unit,
) : InputConnectionWrapper(target, false) {
    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        onUserEdit()
        return super.commitText(text, newCursorPosition)
    }

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        onUserEdit()
        return super.setComposingText(text, newCursorPosition)
    }

    override fun finishComposingText(): Boolean {
        onUserEdit()
        return super.finishComposingText()
    }

    override fun commitCorrection(correctionInfo: CorrectionInfo?): Boolean {
        onUserEdit()
        return super.commitCorrection(correctionInfo)
    }

    override fun deleteSurroundingText(before: Int, after: Int): Boolean {
        onUserEdit()
        return super.deleteSurroundingText(before, after)
    }

    override fun deleteSurroundingTextInCodePoints(before: Int, after: Int): Boolean {
        onUserEdit()
        return super.deleteSurroundingTextInCodePoints(before, after)
    }

    override fun sendKeyEvent(event: KeyEvent?): Boolean {
        onUserEdit()
        return super.sendKeyEvent(event)
    }

    // IMEs on API 33+ (Gboard included) send composing and committed text
    // through the `TextAttribute` overloads, and API 34 adds `replaceText`
    // and stylus handwriting gestures. The framework only calls these on
    // those levels, so the minSdk-21 class never resolves them on older ones.
    @Suppress("NewApi")
    override fun commitText(
        text: CharSequence,
        newCursorPosition: Int,
        textAttribute: TextAttribute?,
    ): Boolean {
        onUserEdit()
        return super.commitText(text, newCursorPosition, textAttribute)
    }

    @Suppress("NewApi")
    override fun setComposingText(
        text: CharSequence,
        newCursorPosition: Int,
        textAttribute: TextAttribute?,
    ): Boolean {
        onUserEdit()
        return super.setComposingText(text, newCursorPosition, textAttribute)
    }

    @Suppress("NewApi")
    override fun replaceText(
        start: Int,
        end: Int,
        text: CharSequence,
        newCursorPosition: Int,
        textAttribute: TextAttribute?,
    ): Boolean {
        onUserEdit()
        return super.replaceText(start, end, text, newCursorPosition, textAttribute)
    }

    @Suppress("NewApi")
    override fun performHandwritingGesture(
        gesture: HandwritingGesture,
        executor: Executor?,
        consumer: IntConsumer?,
    ) {
        onUserEdit()
        super.performHandwritingGesture(gesture, executor, consumer)
    }
}
