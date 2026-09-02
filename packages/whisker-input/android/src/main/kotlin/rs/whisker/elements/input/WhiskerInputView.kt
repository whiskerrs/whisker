// Whisker module element hosting a native EditText. Registration is driven by
// `InputModule`'s `definition()`, not by annotations on this class.
//
// ## Two-way binding + cursor-jump prevention
//
// The Rust side round-trips every `input` event value back down as a
// new `value` prop, so `setValue` is a no-op when the incoming string
// already matches what the EditText displays — otherwise the cursor
// jumps to the end on every keystroke. Two guards keep framework-origin
// writes out of the bound signal: `programmaticWrite` for our own
// writes, and `userEdit`, which `WhiskerEditText` sets at the entry
// points a user's edits actually arrive through.
//
// Text styling arrives through ModuleDefinition.TextStyle, the same contract
// used by every Host.

package rs.whisker.elements.input

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.os.Build
import android.text.Editable
import android.text.InputFilter
import android.text.InputType
import android.text.TextWatcher
import android.util.TypedValue
import android.view.Gravity
import android.view.KeyEvent
import android.view.autofill.AutofillValue
import android.view.inputmethod.CorrectionInfo
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputConnectionWrapper
import android.view.inputmethod.InputMethodManager
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerCustomEvent
import rs.whisker.runtime.WhiskerUI
import rs.whisker.runtime.WhiskerTextAlignment
import rs.whisker.runtime.WhiskerTextStyle

open class WhiskerInputView(context: WhiskerContext) : WhiskerUI<android.widget.EditText>(context) {

    // -------------------------------------------------------------------------
    // State
    // -------------------------------------------------------------------------

    /// True while we are programmatically writing to the EditText, so
    /// `afterTextChanged` doesn't emit an `input` event and double-fire the
    /// two-way round-trip.
    private var programmaticWrite: Boolean = false

    /// True when the edit `afterTextChanged` is about to report came from
    /// the user rather than from the framework. Set at the edit's entry
    /// point (see [WhiskerEditText]) and consumed — always — by the
    /// watcher.
    ///
    /// Android fires a storm of `afterTextChanged('')` callbacks at mount
    /// / IME-attach time that are NOT user edits and NOT covered by
    /// `programmaticWrite` (the system clears/restores the editor as the
    /// InputConnection is established). Ungated, those spurious empty
    /// events flow through the two-way writeback and clobber the bound
    /// signal to "".
    ///
    /// Why not gate on "has the user ever touched this field" instead:
    /// a field the app focused itself (`InputRef::focus`, `auto-focus`)
    /// never receives a touch, and soft-keyboard text — every IME
    /// composition, and every commit on a keyboard that doesn't inject
    /// key events — arrives through the `InputConnection`, not through
    /// `View.onKeyDown`. Under a sticky interaction gate a whole word
    /// could land in the field while the bound signal stayed empty.
    private var userEdit: Boolean = false

    /// Last text the two sides agreed on. A change that leaves the text at
    /// this value carries no information for the Rust signal, so it is not
    /// emitted: that collapses the mount-time `afterTextChanged('')` storm
    /// on an empty field and keeps a `userEdit` tag set by a non-editing
    /// key (arrows, enter) from leaking a later framework write upward.
    private var lastEmitted: String = ""

    /// Pending `auto-focus` request, held until the view attaches to a
    /// window — a focus request on a detached EditText does nothing.
    private var pendingAutoFocus: Boolean = false

    /// Selected auto-capitalization flag bit (one of the
    /// `InputType.TYPE_TEXT_FLAG_CAP_*` flags, or `0` for "none").
    ///
    /// On Android autocapitalization is not a standalone property like
    /// iOS's `autocapitalizationType`; it is flag bits packed into the same
    /// `inputType` Int that carries the class / variation / multiline /
    /// password bits, so `setKeyboardType`, `setSecure`, and `setMultiline`
    /// all wipe it when they rebuild `inputType`. Caching it here and
    /// reapplying via [applyTextFlags] makes the setting survive regardless
    /// of prop-arrival order.
    ///
    /// The `TYPE_TEXT_FLAG_CAP_SENTENCES` default matches iOS UIKit's
    /// `.sentences`.
    private var capFlag: Int = InputType.TYPE_TEXT_FLAG_CAP_SENTENCES

    /// The autocorrect and "no suggestions" bits. Like [capFlag] these live
    /// in the shared `inputType` and are reinstated by [applyTextFlags]
    /// after any rebuild. Defaults match iOS: autocorrect on, suggestions
    /// shown.
    private var autoCorrectFlag: Int = InputType.TYPE_TEXT_FLAG_AUTO_CORRECT
    private var noSuggestionsFlag: Int = 0

    // -------------------------------------------------------------------------
    // View creation
    // -------------------------------------------------------------------------

    override fun createView(context: Context): android.widget.EditText {
        val et = WhiskerEditText(context)
        // Common Whisker presentation paints the background, border, radius,
        // and clipping on the outer module element.
        et.background = null
        et.setPadding(0, 0, 0, 0)
        et.isSingleLine = true
        et.inputType = InputType.TYPE_CLASS_TEXT

        // `setAutoFocus` may run before the EditText is attached, and a
        // focus request has no effect then, so listen on the EditText.
        et.addOnAttachStateChangeListener(
            object : android.view.View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: android.view.View) {
                    if (pendingAutoFocus) {
                        pendingAutoFocus = false
                        focusField()
                    }
                }

                override fun onViewDetachedFromWindow(v: android.view.View) {
                    // Android, unlike UIKit, does not resign the keyboard
                    // target on window removal, so a field unmounted while
                    // focused would linger as the IME target. Route-driven
                    // dismissal is handled up front by whisker-router; this
                    // covers unmounts that aren't route changes.
                    blurField()
                }
            },
        )

        // Hardware-keyboard edits bypass the InputConnection entirely, so
        // tag them here. Returning false leaves the event for the EditText.
        et.setOnKeyListener { _, _, _ ->
            userEdit = true
            false
        }

        et.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                // Consume the tag unconditionally: one tagged entry point
                // authorizes exactly one text change, so a tag left over
                // from a key that edited nothing can't authorize a later
                // framework write.
                val fromUser = userEdit
                userEdit = false
                if (programmaticWrite) return
                if (!fromUser) return
                val text = s?.toString() ?: ""
                if (text == lastEmitted) return
                lastEmitted = text
                emitInput(text)
            }
        })

        et.setOnFocusChangeListener { _, hasFocus ->
            if (hasFocus) {
                emitEvent("focus", "")
            } else {
                emitEvent("change", et.text?.toString() ?: "")
                emitEvent("blur", "")
            }
        }

        et.setOnEditorActionListener { _, actionId, _ ->
            when (actionId) {
                EditorInfo.IME_ACTION_DONE,
                EditorInfo.IME_ACTION_GO,
                EditorInfo.IME_ACTION_SEARCH,
                EditorInfo.IME_ACTION_SEND -> {
                    emitEvent("submit", et.text?.toString() ?: "")
                    true
                }
                else -> false
            }
        }

        return et
    }

    /// EditText that tags every edit the user makes, for [userEdit].
    ///
    /// An `Editable` carries no record of where a change came from, and by
    /// the time `afterTextChanged` runs the originating call is off the
    /// stack — so the distinction has to be captured at the entry points a
    /// user's edits arrive through: the IME's `InputConnection`, the
    /// text-selection toolbar (paste / cut, which writes the `Editable`
    /// directly), and the autofill framework. Everything else that mutates
    /// the editor — Host props, system state restoration, and application
    /// InputConnection is (re)established — stays untagged and out of the
    /// two-way binding.
    private inner class WhiskerEditText(context: Context) : android.widget.EditText(context) {
        override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
            val base = super.onCreateInputConnection(outAttrs) ?: return null
            return object : InputConnectionWrapper(base, false) {
                override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
                    userEdit = true
                    return super.commitText(text, newCursorPosition)
                }

                override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
                    userEdit = true
                    return super.setComposingText(text, newCursorPosition)
                }

                override fun finishComposingText(): Boolean {
                    userEdit = true
                    return super.finishComposingText()
                }

                override fun commitCorrection(correctionInfo: CorrectionInfo?): Boolean {
                    userEdit = true
                    return super.commitCorrection(correctionInfo)
                }

                override fun deleteSurroundingText(before: Int, after: Int): Boolean {
                    userEdit = true
                    return super.deleteSurroundingText(before, after)
                }

                override fun deleteSurroundingTextInCodePoints(before: Int, after: Int): Boolean {
                    userEdit = true
                    return super.deleteSurroundingTextInCodePoints(before, after)
                }

                override fun sendKeyEvent(event: KeyEvent?): Boolean {
                    userEdit = true
                    return super.sendKeyEvent(event)
                }
            }
        }

        override fun onTextContextMenuItem(id: Int): Boolean {
            userEdit = true
            return super.onTextContextMenuItem(id)
        }

        // Overriding an API-26 method on a minSdk-21 class: the framework
        // only ever calls it on 26+, and the signature is resolved lazily.
        @Suppress("NewApi")
        override fun autofill(value: AutofillValue) {
            userEdit = true
            super.autofill(value)
        }
    }

    // -------------------------------------------------------------------------
    // Props called from InputModule
    // -------------------------------------------------------------------------

    /** External `value` module property. */
    fun setValue(incoming: String) {
        applyTextIfChanged(incoming)
    }

    /** `setValue` called from the callable UI method (same guard, same effect). */
    fun setValueExternal(incoming: String) {
        applyTextIfChanged(incoming)
    }

    fun setPlaceholder(text: String) {
        view().hint = text
    }

    fun setPlaceholderColor(color: String) {
        val et = view()
        val parsed = parseColor(color) ?: return
        et.setHintTextColor(parsed)
    }

    // Named `applyCaretColor` to distinguish it from the Android widget API.
    fun applyCaretColor(color: String) {
        val et = view()
        val parsed = parseColor(color) ?: return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            et.textCursorDrawable?.setTint(parsed)
        }
        // Pre-API-29 the caret keeps the theme color: there is no typed
        // cursor-tint API, and the `mCursorDrawableRes` reflection hack is
        // greylisted from API 28. Cosmetic only.
    }

    fun setSelectionColor(color: String) {
        val et = view()
        val parsed = parseColor(color) ?: return
        et.highlightColor = parsed
    }

    fun setMultiline(multiline: Boolean) {
        val et = view()
        if (multiline) {
            et.isSingleLine = false
            et.inputType = et.inputType or InputType.TYPE_TEXT_FLAG_MULTI_LINE
            // Top-align, matching iOS.
            et.gravity = Gravity.TOP or (et.gravity and Gravity.HORIZONTAL_GRAVITY_MASK)
        } else {
            et.isSingleLine = true
            et.inputType = et.inputType and InputType.TYPE_TEXT_FLAG_MULTI_LINE.inv()
            et.gravity = Gravity.CENTER_VERTICAL or (et.gravity and Gravity.HORIZONTAL_GRAVITY_MASK)
        }
        // `isSingleLine` / the MULTI_LINE toggle resets inputType bits.
        applyTextFlags()
    }

    fun setLines(count: Long) {
        val et = view()
        val n = count.coerceIn(0, Int.MAX_VALUE.toLong()).toInt()
        if (n > 0) {
            // CSS is the authoritative height; this is best-effort.
            et.setLines(n)
        }
    }

    fun setSecure(secure: Boolean) {
        val et = view()
        if (secure) {
            // Preserve the current class (text vs number), replace variation.
            val base = et.inputType and InputType.TYPE_MASK_CLASS
            et.inputType = base or InputType.TYPE_TEXT_VARIATION_PASSWORD
        } else {
            val base = et.inputType and InputType.TYPE_MASK_CLASS
            et.inputType = base or InputType.TYPE_TEXT_VARIATION_NORMAL
        }
        // The masked rebuild above drops the cap flags.
        applyTextFlags()
    }

    fun setEditable(editable: Boolean) {
        val et = view()
        et.isEnabled = editable
        et.isFocusable = editable
        et.isFocusableInTouchMode = editable
    }

    fun setAutoFocus(autoFocus: Boolean) {
        if (!autoFocus) {
            pendingAutoFocus = false
            return
        }
        val et = view()
        if (et.isAttachedToWindow) {
            focusField()
        } else {
            pendingAutoFocus = true
        }
    }

    fun setMaxLength(count: Long) {
        val et = view()
        val n = count.coerceIn(0, Int.MAX_VALUE.toLong()).toInt()
        if (n > 0) {
            et.filters = arrayOf(InputFilter.LengthFilter(n))
        } else {
            et.filters = emptyArray()
        }
    }

    fun setKeyboardType(type: String) {
        val et = view()
        // Preserve the variation flags (password, etc.), replace the class
        // bits.
        val variation = et.inputType and InputType.TYPE_MASK_VARIATION
        et.inputType = when (type) {
            "number" -> InputType.TYPE_CLASS_NUMBER or variation
            "decimal" -> InputType.TYPE_CLASS_NUMBER or
                InputType.TYPE_NUMBER_FLAG_DECIMAL or variation
            "email" -> InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
            "phone" -> InputType.TYPE_CLASS_PHONE or variation
            "url" -> InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_VARIATION_URI
            else -> InputType.TYPE_CLASS_TEXT or variation
        }
        // Rebuilding `inputType` from scratch drops the cap flags.
        applyTextFlags()
    }

    fun setReturnKey(type: String) {
        val et = view()
        et.imeOptions = when (type) {
            "done" -> EditorInfo.IME_ACTION_DONE
            "go" -> EditorInfo.IME_ACTION_GO
            "next" -> EditorInfo.IME_ACTION_NEXT
            "search" -> EditorInfo.IME_ACTION_SEARCH
            "send" -> EditorInfo.IME_ACTION_SEND
            else -> EditorInfo.IME_ACTION_UNSPECIFIED
        }
    }

    fun setAutoCapitalize(mode: String) {
        capFlag = when (mode) {
            "none" -> 0
            "words" -> InputType.TYPE_TEXT_FLAG_CAP_WORDS
            "characters" -> InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS
            else -> InputType.TYPE_TEXT_FLAG_CAP_SENTENCES // "sentences"
        }
        applyTextFlags()
    }

    fun setAutocorrect(enabled: Boolean) {
        autoCorrectFlag = if (enabled) InputType.TYPE_TEXT_FLAG_AUTO_CORRECT else 0
        applyTextFlags()
    }

    fun setSpellCheck(enabled: Boolean) {
        // `spell_check` is the inverse of the `NO_SUGGESTIONS` flag.
        noSuggestionsFlag = if (enabled) 0 else InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        applyTextFlags()
    }

    /**
     * Reapply the cached text-behaviour flag bits ([capFlag],
     * [autoCorrectFlag], [noSuggestionsFlag]) onto the EditText's current
     * `inputType`, clearing the managed bits first. Every setter that
     * rebuilds `inputType` ([setKeyboardType], [setSecure],
     * [setMultiline]) must call this, since all three settings share that
     * one Int (unlike iOS's orthogonal traits).
     *
     * The flags only have an effect under `TYPE_CLASS_TEXT`; Android
     * ignores them for number / phone classes, so ORing unconditionally is
     * harmless.
     */
    private fun applyTextFlags() {
        val et = view()
        val managed = InputType.TYPE_TEXT_FLAG_CAP_SENTENCES or
            InputType.TYPE_TEXT_FLAG_CAP_WORDS or
            InputType.TYPE_TEXT_FLAG_CAP_CHARACTERS or
            InputType.TYPE_TEXT_FLAG_AUTO_CORRECT or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        et.inputType = (et.inputType and managed.inv()) or
            capFlag or autoCorrectFlag or noSuggestionsFlag
    }

    // -------------------------------------------------------------------------
    // Callable UI methods (invoked from InputModule's Function blocks)
    // -------------------------------------------------------------------------

    fun focusField() {
        val et = view()
        et.requestFocus()
        val imm = et.context.getSystemService(Context.INPUT_METHOD_SERVICE)
            as? InputMethodManager ?: return
        imm.showSoftInput(et, InputMethodManager.SHOW_IMPLICIT)
    }

    fun blurField() {
        val et = view()
        et.clearFocus()
        val imm = et.context.getSystemService(Context.INPUT_METHOD_SERVICE)
            as? InputMethodManager ?: return
        imm.hideSoftInputFromWindow(et.windowToken, 0)
    }

    /** Clear the text and emit `input` so the bound signal updates. */
    fun clearField() {
        val et = view()
        // An explicit, app-initiated content change: the `input` event has
        // to fire, but it is not a *user* edit, so it can't reach the
        // watcher's gate. Write under `programmaticWrite` and emit here
        // instead — which also makes the emit unconditional, so clearing an
        // already-empty field still tells the Rust side it is empty.
        programmaticWrite = true
        try {
            et.setText("")
            et.setSelection(0)
        } finally {
            programmaticWrite = false
        }
        lastEmitted = ""
        emitInput("")
    }

    // -------------------------------------------------------------------------
    // Resolved text style
    // -------------------------------------------------------------------------

    fun applyTextStyle(style: WhiskerTextStyle) {
        val et = view()
        et.setTextColor(style.color)
        et.setTextSize(TypedValue.COMPLEX_UNIT_PX, style.fontSize)
        val family = style.fontFamilies.firstOrNull()?.takeUnless { it == "system" }
        val base = Typeface.create(family, Typeface.NORMAL)
        et.typeface = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            Typeface.create(base, style.fontWeight.coerceIn(1, 1000), false)
        } else {
            Typeface.create(base, if (style.fontWeight >= 600) Typeface.BOLD else Typeface.NORMAL)
        }
        val horizontal = when (style.alignment) {
            WhiskerTextAlignment.CENTER -> Gravity.CENTER_HORIZONTAL
            WhiskerTextAlignment.END, WhiskerTextAlignment.RIGHT -> Gravity.END
            WhiskerTextAlignment.START, WhiskerTextAlignment.LEFT -> Gravity.START
        }
        et.gravity = (et.gravity and Gravity.VERTICAL_GRAVITY_MASK) or horizontal
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /**
     * Set the EditText text to [incoming] only when it differs from what is
     * currently displayed — an unconditional write would jump the cursor to
     * the end on every keystroke of the two-way round-trip.
     */
    private fun applyTextIfChanged(incoming: String) {
        val et = view()
        lastEmitted = incoming
        val current = et.text?.toString() ?: ""
        if (current == incoming) return
        programmaticWrite = true
        try {
            et.setText(incoming)
            et.setSelection(et.text?.length ?: 0)
        } finally {
            programmaticWrite = false
        }
    }

    /**
     * Dispatch a custom event carrying the current text, in the shape
     * `InputEvent` deserializes on the Rust side. `focus` and `blur` pass
     * an empty [text] and the Rust handler ignores it.
     *
     * Dispatch is synchronous, which is safe because `with_renderer` takes
     * a shared borrow and scopes every renderer field borrow so it never
     * spans a re-entrant FFI call (whisker #3). All callers run on the UI
     * thread.
     */
    private fun emitEvent(name: String, text: String) {
        // Pass the payload directly; the Host event boundary owns the common
        // event envelope.
        val params = mapOf("value" to text)
        WhiskerCustomEvent.dispatch(
            ui = this,
            name = name,
            params = params,
        )
    }

    /** Convenience for the TextWatcher `afterTextChanged` path. */
    private fun emitInput(text: String) = emitEvent("input", text)

    /**
     * Parse a CSS color string. Handles `#RGB`, `#RRGGBB`, and
     * `#AARRGGBB`. Falls back to `null` for unrecognised strings so
     * the caller can skip the assignment rather than throwing.
     */
    private fun parseColor(color: String): Int? {
        if (color.isBlank()) return null
        return try {
            Color.parseColor(color)
        } catch (_: Throwable) {
            null
        }
    }
}
