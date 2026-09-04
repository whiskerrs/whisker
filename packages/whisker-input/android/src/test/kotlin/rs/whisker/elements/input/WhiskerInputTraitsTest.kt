package rs.whisker.elements.input

import android.text.InputType
import org.junit.Assert.assertEquals
import org.junit.Test

class WhiskerInputTraitsTest {
    @Test
    fun multilineSurvivesLaterInputTypeRebuilds() {
        val afterSecureOrKeyboardType = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_VARIATION_NORMAL

        val actual = inputTypeWithManagedFlags(
            inputType = afterSecureOrKeyboardType,
            multiline = true,
            capFlag = InputType.TYPE_TEXT_FLAG_CAP_SENTENCES,
            autoCorrectFlag = InputType.TYPE_TEXT_FLAG_AUTO_CORRECT,
            noSuggestionsFlag = 0,
        )

        assertEquals(
            InputType.TYPE_TEXT_FLAG_MULTI_LINE,
            actual and InputType.TYPE_TEXT_FLAG_MULTI_LINE,
        )
    }

    @Test
    fun singleLineClearsAStaleMultilineFlag() {
        val actual = inputTypeWithManagedFlags(
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE,
            multiline = false,
            capFlag = 0,
            autoCorrectFlag = 0,
            noSuggestionsFlag = 0,
        )

        assertEquals(0, actual and InputType.TYPE_TEXT_FLAG_MULTI_LINE)
    }
}
