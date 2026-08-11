// Lynx UI subclass hosting a native EditText. Registration is driven by
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
// ## CSS text-style interception
//
// Whisker-registered custom UIs don't get an APT-generated prop setter
// for CSS properties, so `color`, `font-size`, `font-weight`, and
// `text-align` are intercepted in `updatePropertiesInterval` via
// `StylesDiffMap.mBackingMap` — the same pattern `WhiskerImageView`
// uses for `border-radius`.

package rs.whisker.elements.input

import android.content.Context
import android.graphics.Color
import android.graphics.PorterDuff
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
import com.lynx.tasm.behavior.StylesDiffMap
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerCustomEvent
import rs.whisker.runtime.WhiskerUI

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

    /// Last-applied CSS `background-color` (ARGB int) and corner radius
    /// (device px), rendered through a [GradientDrawable] we own rather
    /// than Lynx's `BackgroundDrawable` — see [applyBackground].
    private var bgColor: Int = Color.TRANSPARENT
    private var bgRadiusPx: Float = 0f

    // -------------------------------------------------------------------------
    // View creation
    // -------------------------------------------------------------------------

    override fun createView(context: Context): android.widget.EditText {
        val et = WhiskerEditText(context)
        // A GradientDrawable we own, replacing the EditText's default
        // underline. It must not be nulled: Lynx calls
        // `view.setBackground(...)` exactly once, in
        // `LynxUI.didEnsureCreateView()`, and only if its own
        // `BackgroundDrawable` already exists at that instant — later
        // background-color / border-radius changes mutate that drawable in
        // place without re-attaching it, so a custom UI's CSS background
        // never reaches the wrapped view on its own.
        et.background = android.graphics.drawable.GradientDrawable()
        et.isSingleLine = true
        et.inputType = InputType.TYPE_CLASS_TEXT

        // `setAutoFocus` may run before the EditText is attached, and a
        // focus request has no effect then. This class is the LynxUI
        // wrapper, not the View, so `onAttachedToWindow` isn't overridable
        // here — listen on the EditText instead.
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
    /// the editor — Lynx applying props, the system restoring state as the
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

    /** External `value` prop (from Lynx attribute pipeline). */
    fun setValue(incoming: String) {
        applyTextIfChanged(incoming)
    }

    /** `setValue` called from the callable UI method (same guard, same effect). */
    fun setValueExternal(incoming: String) {
        applyTextIfChanged(incoming)
    }

    fun setPlaceholder(text: String) {
        view?.hint = text
    }

    fun setPlaceholderColor(color: String) {
        val et = view ?: return
        val parsed = parseColor(color) ?: return
        et.setHintTextColor(parsed)
    }

    // Named `applyCaretColor`, not `setCaretColor`: the LynxUI base class
    // already declares `setCaretColor(String?)`, and a same-JVM-signature
    // Kotlin `setCaretColor(String)` would accidentally override it.
    // `InputModule`'s `caret-color` Prop calls this instead.
    fun applyCaretColor(color: String) {
        val et = view ?: return
        val parsed = parseColor(color) ?: return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            et.textCursorDrawable?.setColorFilter(parsed, PorterDuff.Mode.SRC_IN)
        }
        // Pre-API-29 the caret keeps the theme color: there is no typed
        // cursor-tint API, and the `mCursorDrawableRes` reflection hack is
        // greylisted from API 28. Cosmetic only.
    }

    fun setSelectionColor(color: String) {
        val et = view ?: return
        val parsed = parseColor(color) ?: return
        et.highlightColor = parsed
    }

    fun setMultiline(flag: String) {
        val et = view ?: return
        val multi = flag == "true"
        if (multi) {
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

    fun setLines(countStr: String) {
        val et = view ?: return
        val n = countStr.toIntOrNull() ?: 0
        if (n > 0) {
            // CSS is the authoritative height; this is best-effort.
            et.setLines(n)
        }
    }

    fun setSecure(flag: String) {
        val et = view ?: return
        if (flag == "true") {
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

    fun setEditable(flag: String) {
        val et = view ?: return
        val enabled = flag != "false"
        et.isEnabled = enabled
        et.isFocusable = enabled
        et.isFocusableInTouchMode = enabled
    }

    fun setAutoFocus(flag: String) {
        if (flag != "true") return
        val et = view ?: return
        if (et.isAttachedToWindow) {
            focusField()
        } else {
            pendingAutoFocus = true
        }
    }

    fun setMaxLength(countStr: String) {
        val et = view ?: return
        val n = countStr.toIntOrNull() ?: 0
        if (n > 0) {
            et.filters = arrayOf(InputFilter.LengthFilter(n))
        } else {
            et.filters = emptyArray()
        }
    }

    fun setKeyboardType(type: String) {
        val et = view ?: return
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
        val et = view ?: return
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

    fun setAutocorrect(flag: String) {
        autoCorrectFlag = if (flag == "false") 0 else InputType.TYPE_TEXT_FLAG_AUTO_CORRECT
        applyTextFlags()
    }

    fun setSpellCheck(flag: String) {
        // `spell_check` is the inverse of the `NO_SUGGESTIONS` flag.
        noSuggestionsFlag = if (flag == "false") InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS else 0
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
        val et = view ?: return
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
        val et = view ?: return
        et.requestFocus()
        val imm = et.context.getSystemService(Context.INPUT_METHOD_SERVICE)
            as? InputMethodManager ?: return
        imm.showSoftInput(et, InputMethodManager.SHOW_IMPLICIT)
    }

    fun blurField() {
        val et = view ?: return
        et.clearFocus()
        val imm = et.context.getSystemService(Context.INPUT_METHOD_SERVICE)
            as? InputMethodManager ?: return
        imm.hideSoftInputFromWindow(et.windowToken, 0)
    }

    /** Clear the text and emit `input` so the bound signal updates. */
    fun clearField() {
        val et = view ?: return
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

    /** Return the EditText's current text — used by `getValue`. */
    fun currentText(): String = view?.text?.toString() ?: ""

    // -------------------------------------------------------------------------
    // CSS text-style interception
    // -------------------------------------------------------------------------

    /// Custom UIs don't receive an APT-generated prop setter for CSS
    /// properties, but the parsed values do land in
    /// `StylesDiffMap.mBackingMap` keyed by the CSS property name — so the
    /// text styles have to be picked out of the backing map here.
    override fun updatePropertiesInterval(props: StylesDiffMap?) {
        super.updatePropertiesInterval(props)
        val map = props?.mBackingMap ?: return
        val et = view ?: return

        // ARGB int from Lynx's CSS engine.
        if (map.hasKey("color")) {
            runCatching {
                val color = map.getInt("color")
                et.setTextColor(color)
            }
        }

        // PlatformLength quartet `[px, unit, px, unit]`; index 0 is already
        // density-multiplied, so it goes out as COMPLEX_UNIT_PX to avoid
        // double-scaling.
        if (map.hasKey("font-size")) {
            runCatching {
                val arr = map.getArray("font-size")
                if (arr != null && arr.size() >= 1) {
                    val px = arr.getDouble(0).toFloat()
                    et.setTextSize(TypedValue.COMPLEX_UNIT_PX, px)
                }
            }
        }

        // Arrives as a number (400, 700) or a string ("bold", "normal").
        if (map.hasKey("font-weight")) {
            runCatching {
                val weight = when {
                    map.getDynamic("font-weight")?.type ==
                        com.lynx.react.bridge.ReadableType.Number ->
                        map.getInt("font-weight")
                    map.getDynamic("font-weight")?.type ==
                        com.lynx.react.bridge.ReadableType.String ->
                        when (map.getString("font-weight")) {
                            "bold" -> 700
                            "normal" -> 400
                            else -> map.getString("font-weight")?.toIntOrNull() ?: 400
                        }
                    else -> 400
                }
                val style = if (weight >= 600) Typeface.BOLD else Typeface.NORMAL
                et.setTypeface(et.typeface, style)
            }
        }

        if (map.hasKey("text-align")) {
            runCatching {
                val align = map.getString("text-align")
                val hGrav = when (align) {
                    "center" -> Gravity.CENTER_HORIZONTAL
                    "right" -> Gravity.END
                    else -> Gravity.START
                }
                // Preserve the vertical gravity `setMultiline` chose.
                val vGrav = et.gravity and Gravity.VERTICAL_GRAVITY_MASK
                et.gravity = vGrav or hGrav
            }
        }

        var bgChanged = false

        // ARGB int, same encoding as `color`.
        if (map.hasKey("background-color")) {
            runCatching {
                val c = map.getInt("background-color")
                if (c != bgColor) {
                    bgColor = c
                    bgChanged = true
                }
            }
        }

        // Lynx splits the shorthand into four per-corner keys, but
        // GradientDrawable's `cornerRadius` is one uniform float — collapse
        // to the largest corner.
        var maxRadius = 0f
        var sawRadius = false
        for (k in CORNER_KEYS) {
            if (!map.hasKey(k)) continue
            runCatching {
                val arr = map.getArray(k) ?: return@runCatching
                if (arr.size() < 1) return@runCatching
                sawRadius = true
                val px = arr.getDouble(0).toFloat()
                if (px > maxRadius) maxRadius = px
            }
        }
        if (sawRadius && maxRadius != bgRadiusPx) {
            bgRadiusPx = maxRadius
            bgChanged = true
        }

        if (bgChanged) applyBackground()
    }

    /**
     * Rebuild and apply the EditText's background [GradientDrawable] from
     * the current [bgColor] + [bgRadiusPx].
     *
     * A custom Lynx Android UI does NOT get its CSS `background-color` /
     * `border-radius` auto-painted onto the wrapped view: `LynxUI` sets the
     * view background once in `didEnsureCreateView`, and only when its own
     * `BackgroundDrawable` already exists — later mutations don't
     * re-attach. iOS paints it via the UITextField's layer, so this is
     * Android-only work.
     */
    private fun applyBackground() {
        val et = view ?: return
        val bg = android.graphics.drawable.GradientDrawable().apply {
            setColor(bgColor)
            cornerRadius = bgRadiusPx
        }
        et.background = bg
    }

    // -------------------------------------------------------------------------
    // CSS padding
    // -------------------------------------------------------------------------

    /// Lynx resolves CSS `padding` (shorthand, units, %) during the layout
    /// pass, so the values are only final once `onLayoutUpdated` fires;
    /// reading them in `updatePropertiesInterval` can catch a pre-layout
    /// zero on first render.
    override fun onLayoutUpdated() {
        super.onLayoutUpdated()
        applyPadding()
    }

    /**
     * Mirror the Lynx-computed CSS padding (device px) onto the EditText.
     * All four sides are set unconditionally so the EditText's built-in
     * ~4-6dp internal padding never leaks through: with no CSS padding the
     * computed values are 0 and the field sits flush, matching iOS.
     */
    private fun applyPadding() {
        val et = view ?: return
        // These resolve to `LynxBaseUI.getPaddingLeft()` etc. — Lynx's
        // computed padding — not `android.view.View`'s, because `this` is
        // the LynxUI wrapper rather than the View.
        et.setPadding(
            getPaddingLeft(),
            getPaddingTop(),
            getPaddingRight(),
            getPaddingBottom(),
        )
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
        val et = view ?: return
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
     * spans a re-entrant FFI call (whisker #3) — a native focus-loss
     * callback firing `blur` / `change` from inside a Lynx `remove_child`
     * therefore cannot hit "RefCell already borrowed". All callers already
     * run on the UI thread.
     */
    private fun emitEvent(name: String, text: String) {
        // Pass the payload directly as the params — do NOT wrap it in a
        // `detail` key. WhiskerView's LynxEventReporter already places the
        // dispatched params under `detail` in the event body, so wrapping
        // here double-nests and `on_input` only ever sees an empty string.
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

    private companion object {
        /// The four per-corner keys Lynx splits `border-radius` into, each
        /// a `[px, unit, px, unit]` PlatformLength quartet with the
        /// density-multiplied px at index 0.
        val CORNER_KEYS = listOf(
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        )
    }
}
