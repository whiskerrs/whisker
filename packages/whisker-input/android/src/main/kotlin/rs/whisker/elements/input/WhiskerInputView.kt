// Lynx UI subclass hosting a native EditText.
// A plain `WhiskerUI` subclass — no Whisker annotations; registration
// is driven by `InputModule`'s `definition()` (see `InputModule.kt`).
//
// ## Two-way binding + cursor-jump prevention
//
// The Rust side round-trips every `input` event value back down as a
// new `value` prop. To avoid the cursor jumping to the end on every
// keystroke, `setValue` is a no-op when the incoming string already
// matches what the EditText displays. A boolean `programmaticWrite`
// guard suppresses `afterTextChanged` during external writes so the
// Rust signal doesn't double-fire.
//
// ## CSS text-style interception
//
// `color`, `font-size`, `font-weight`, and `text-align` arrive through
// Lynx's CSS cascade. Whisker-registered custom UIs don't get an
// APT-generated prop setter for CSS properties, so we intercept them
// in `updatePropertiesInterval` via `StylesDiffMap.mBackingMap` — the
// same pattern used by `WhiskerImageView` for `border-radius`.

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
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import com.lynx.tasm.behavior.StylesDiffMap
import rs.whisker.runtime.WhiskerContext
import rs.whisker.runtime.WhiskerCustomEvent
import rs.whisker.runtime.WhiskerUI

open class WhiskerInputView(context: WhiskerContext) : WhiskerUI<android.widget.EditText>(context) {

    // -------------------------------------------------------------------------
    // State
    // -------------------------------------------------------------------------

    /// True while we are programmatically writing to the EditText (e.g.
    /// from an external `value` prop update or `setValue` / `clear` call).
    /// While this flag is set, `afterTextChanged` does NOT emit an `input`
    /// event so the two-way round-trip doesn't double-fire.
    private var programmaticWrite: Boolean = false

    /// True once the user has actually interacted with the field (a tap
    /// or a hardware key press). Until then we suppress `input` emission.
    ///
    /// Android fires a storm of `afterTextChanged('')` callbacks at mount
    /// / IME-attach time that are NOT user edits and NOT covered by
    /// `programmaticWrite` (they come from the system clearing/restoring
    /// the editor as the InputConnection is established). Without this
    /// gate, those spurious empty events flow through the two-way
    /// writeback and clobber the bound signal to "". A genuine edit can
    /// only happen after the user taps to focus (or presses a key), so
    /// gating on real interaction drops the spurious mount-time events
    /// while still emitting every real keystroke.
    private var userInteracted: Boolean = false

    /// Pending `auto-focus` request. Stored until the view attaches to a
    /// window (EditText must be attached before requesting focus works).
    private var pendingAutoFocus: Boolean = false

    /// Last-applied CSS `background-color` (ARGB int) and corner radius
    /// (device px). We render these ourselves via a [GradientDrawable]
    /// rather than relying on Lynx's `BackgroundDrawable` — see
    /// [applyBackground] for why. `null` color means "transparent".
    private var bgColor: Int = Color.TRANSPARENT
    private var bgRadiusPx: Float = 0f

    // -------------------------------------------------------------------------
    // View creation
    // -------------------------------------------------------------------------

    override fun createView(context: Context): android.widget.EditText {
        val et = android.widget.EditText(context)
        // Replace the EditText's default underline background with a
        // GradientDrawable WE own, so the CSS `background-color` /
        // `border-radius` from the style cascade render here. We must NOT
        // hard-null the background: a custom Lynx Android UI does not get
        // its CSS background auto-drawn onto the wrapped view the way iOS
        // does — Lynx only calls `view.setBackground(...)` once, in
        // `LynxUI.didEnsureCreateView()`, and only if its own
        // `BackgroundDrawable` already exists at that instant; later
        // background-color / border-radius changes mutate that drawable
        // in place without re-attaching it. Owning the drawable ourselves
        // (fed from `updatePropertiesInterval`, the same channel as the
        // text color) is deterministic. Starts transparent (no CSS
        // background → no visible surface, matching iOS).
        et.background = android.graphics.drawable.GradientDrawable()
        // Single-line by default; `setMultiline("true")` switches to a
        // multiline area.
        et.isSingleLine = true
        et.inputType = InputType.TYPE_CLASS_TEXT

        // Deferred auto-focus: `setAutoFocus` may run before the EditText
        // is attached to a window (a focus request has no effect then).
        // We can't override `onAttachedToWindow` on this class — it's a
        // LynxUI wrapper (`WhiskerUI`), not the View — so listen on the
        // EditText itself.
        et.addOnAttachStateChangeListener(
            object : android.view.View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: android.view.View) {
                    if (pendingAutoFocus) {
                        pendingAutoFocus = false
                        focusField()
                    }
                }

                override fun onViewDetachedFromWindow(v: android.view.View) {}
            },
        )

        // Mark genuine user interaction so the spurious mount/IME
        // `afterTextChanged('')` storm doesn't get emitted as `input`.
        // A tap (to focus) or a hardware key press both count. Neither
        // listener consumes the event (returns false) — the EditText
        // handles it normally.
        et.setOnTouchListener { _, ev ->
            if (ev.action == android.view.MotionEvent.ACTION_UP) {
                userInteracted = true
            }
            false
        }
        et.setOnKeyListener { _, _, _ ->
            userInteracted = true
            false
        }

        // Wire text changes back to Rust.
        et.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}
            override fun afterTextChanged(s: Editable?) {
                // Suppress events that originated from our own programmatic
                // write — only user typing should emit `input`.
                if (programmaticWrite) return
                // Suppress the spurious mount/IME-attach empty-text storm
                // (see `userInteracted`): only emit once the user has
                // actually touched/typed in the field.
                if (!userInteracted) return
                emitInput(s?.toString() ?: "")
            }
        })

        // Focus change → `focus` / `blur` events.
        et.setOnFocusChangeListener { _, hasFocus ->
            if (hasFocus) {
                emitEvent("focus", "")
            } else {
                emitEvent("change", et.text?.toString() ?: "")
                emitEvent("blur", "")
            }
        }

        // Done / Go / Search / Send action → `submit` event.
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

    // Named `applyCaretColor` (not `setCaretColor`) on purpose: the
    // LynxUI base class already declares `setCaretColor(String?)`, and a
    // same-JVM-signature Kotlin `setCaretColor(String)` is an accidental
    // override. We drive the EditText's cursor tint ourselves, so use a
    // distinct name and call it from `InputModule`'s `caret-color` Prop.
    fun applyCaretColor(color: String) {
        val et = view ?: return
        val parsed = parseColor(color) ?: return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            // API 29+: tint the cursor drawable directly.
            et.textCursorDrawable?.setColorFilter(parsed, PorterDuff.Mode.SRC_IN)
        } else {
            // Pre-API-29: attempt reflection to reach mCursorDrawableRes.
            // Best-effort — silently skip on failure.
            try {
                val field = android.widget.TextView::class.java
                    .getDeclaredField("mCursorDrawableRes")
                field.isAccessible = true
                // Not possible to tint without the typed API; skip on older
                // Android. Cursor color is a cosmetic enhancement only.
            } catch (_: Throwable) { }
        }
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
            // Remove single-line constraint, allow multi-line input.
            et.isSingleLine = false
            et.inputType = et.inputType or InputType.TYPE_TEXT_FLAG_MULTI_LINE
            // Top-align text in the multiline area (matches iOS behaviour).
            et.gravity = Gravity.TOP or (et.gravity and Gravity.HORIZONTAL_GRAVITY_MASK)
        } else {
            et.isSingleLine = true
            et.inputType = et.inputType and InputType.TYPE_TEXT_FLAG_MULTI_LINE.inv()
            et.gravity = Gravity.CENTER_VERTICAL or (et.gravity and Gravity.HORIZONTAL_GRAVITY_MASK)
        }
    }

    fun setLines(countStr: String) {
        val et = view ?: return
        val n = countStr.toIntOrNull() ?: 0
        if (n > 0) {
            // Authoritative height is CSS for v1; `setLines` is best-effort.
            et.setLines(n)
        }
    }

    fun setSecure(flag: String) {
        val et = view ?: return
        if (flag == "true") {
            // Preserve current class (text vs number), replace variation.
            val base = et.inputType and InputType.TYPE_MASK_CLASS
            et.inputType = base or InputType.TYPE_TEXT_VARIATION_PASSWORD
        } else {
            val base = et.inputType and InputType.TYPE_MASK_CLASS
            et.inputType = base or InputType.TYPE_TEXT_VARIATION_NORMAL
        }
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
            // Defer until onAttachedToWindow fires.
            pendingAutoFocus = true
        }
    }

    fun setMaxLength(countStr: String) {
        val et = view ?: return
        val n = countStr.toIntOrNull() ?: 0
        if (n > 0) {
            et.filters = arrayOf(InputFilter.LengthFilter(n))
        } else {
            // Remove any previously applied length filter.
            et.filters = emptyArray()
        }
    }

    fun setKeyboardType(type: String) {
        val et = view ?: return
        // Preserve the variation flags (password, etc.) and replace
        // only the class bits.
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
        // Do NOT set `programmaticWrite = true` here — we WANT the
        // `input` event to fire so the Rust signal sees the empty value.
        // `clear()` is an explicit, app-initiated content change, so let
        // its emit through the `userInteracted` gate (and treat the field
        // as active from here on).
        userInteracted = true
        et.setText("")
        // Move the cursor to position 0 (end of empty string).
        et.setSelection(0)
    }

    /** Return the EditText's current text — used by `getValue`. */
    fun currentText(): String = view?.text?.toString() ?: ""

    // -------------------------------------------------------------------------
    // CSS text-style interception
    // -------------------------------------------------------------------------

    /// Intercept the CSS text-style cascade before the base implementation
    /// runs. Custom UIs don't receive an APT-generated prop setter for CSS
    /// properties, but the parsed values DO land in
    /// `StylesDiffMap.mBackingMap` keyed by the CSS property name. We pull
    /// `color`, `font-size`, `font-weight`, and `text-align` out of the
    /// backing map and apply them to the EditText.
    ///
    /// `font-size` arrives as a 4-element `[px, unit, px, unit]` quartet
    /// (PlatformLength) matching the border-radius shape in WhiskerImageView;
    /// index 0 is the already-density-multiplied pixel value. We pass it to
    /// `setTextSize(COMPLEX_UNIT_PX, px)` to avoid double-scaling.
    ///
    /// `color` and `font-weight` arrive as scalars (int and string/int).
    /// `text-align` arrives as a string ("left", "center", "right").
    override fun updatePropertiesInterval(props: StylesDiffMap?) {
        super.updatePropertiesInterval(props)
        val map = props?.mBackingMap ?: return
        val et = view ?: return

        // color — arrives as an ARGB int from Lynx's CSS engine.
        if (map.hasKey("color")) {
            runCatching {
                val color = map.getInt("color")
                et.setTextColor(color)
            }
        }

        // font-size — PlatformLength quartet: [px, unit, px, unit].
        // Index 0 is the pre-multiplied pixel value.
        if (map.hasKey("font-size")) {
            runCatching {
                val arr = map.getArray("font-size")
                if (arr != null && arr.size() >= 1) {
                    val px = arr.getDouble(0).toFloat()
                    et.setTextSize(TypedValue.COMPLEX_UNIT_PX, px)
                }
            }
        }

        // font-weight — may arrive as a number (400, 700) or a string
        // ("bold", "normal"). Map to Typeface style.
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

        // text-align — "left" / "center" / "right".
        if (map.hasKey("text-align")) {
            runCatching {
                val align = map.getString("text-align")
                val hGrav = when (align) {
                    "center" -> Gravity.CENTER_HORIZONTAL
                    "right" -> Gravity.END
                    else -> Gravity.START
                }
                // Preserve the vertical gravity already set (e.g. TOP for
                // multiline, CENTER_VERTICAL for single-line).
                val vGrav = et.gravity and Gravity.VERTICAL_GRAVITY_MASK
                et.gravity = vGrav or hGrav
            }
        }

        // background-color + border-radius — see [applyBackground] for the
        // rationale (Lynx doesn't auto-draw a custom UI's CSS background
        // onto the wrapped Android view). Read both from the same backing
        // map and rebuild our GradientDrawable when either changes.
        var bgChanged = false

        // `background-color` arrives as an ARGB int (same encoding as
        // `color`, matching Lynx's `setBackgroundColor(int)` setter).
        if (map.hasKey("background-color")) {
            runCatching {
                val c = map.getInt("background-color")
                if (c != bgColor) {
                    bgColor = c
                    bgChanged = true
                }
            }
        }

        // `border-*-radius` arrive as PlatformLength quartets
        // `[px, unit, px, unit]` (index 0 = density-multiplied px), the
        // same shape WhiskerImageView reads. Lynx splits the shorthand
        // into four per-corner keys; GradientDrawable's `cornerRadius`
        // takes one uniform float, so we collapse to the largest corner.
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
     * We draw the CSS background ourselves because a custom Lynx Android
     * UI does NOT get its CSS `background-color` / `border-radius`
     * auto-painted onto the wrapped view (the iOS path does this via the
     * UITextField's layer; the Android `LynxUI` only sets the view
     * background once in `didEnsureCreateView`, and only when its own
     * `BackgroundDrawable` already exists — later mutations don't
     * re-attach). The GradientDrawable also replaces the EditText's
     * default underline, which is what we wanted gone anyway.
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

    /// Lynx resolves the element's CSS `padding` / `padding-*` (shorthand,
    /// units, %) during the layout pass and stores the result, in device
    /// pixels, on the `LynxBaseUI` base via `mPaddingLeft` / `mPaddingTop`
    /// / `mPaddingRight` / `mPaddingBottom` (exposed through
    /// `getPaddingLeft()` etc.). `onLayoutUpdated` fires after every layout
    /// pass with those values final, so it's the right hook to mirror them
    /// onto the EditText — reading them in `updatePropertiesInterval` could
    /// catch a pre-layout (stale/zero) value on first render.
    ///
    /// NOTE the name clash: `getPaddingLeft()` here resolves to LYNX's
    /// computed padding (`LynxBaseUI.getPaddingLeft()`, returning
    /// `mPaddingLeft`), NOT `android.view.View.getPaddingLeft()` — `this`
    /// is the LynxUI wrapper, not the View.
    override fun onLayoutUpdated() {
        super.onLayoutUpdated()
        applyPadding()
    }

    /**
     * Mirror the Lynx-computed CSS padding (device px) onto the EditText,
     * making CSS the single source of truth and overriding the EditText's
     * built-in default internal padding (~4-6dp). With no CSS padding the
     * computed values are 0 → the field sits flush, matching iOS's 0
     * default. The EditText default must never leak through, so we set all
     * four sides unconditionally.
     */
    private fun applyPadding() {
        val et = view ?: return
        // `getPaddingLeft()` / … are LynxBaseUI's computed-padding
        // accessors (device px), not the EditText's own.
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
     * Set the EditText text to [incoming] only when it differs from what
     * is currently displayed. When a write is needed, `programmaticWrite`
     * is set to suppress the resulting `afterTextChanged` `input` event
     * (it's not user-typed). Cursor is moved to the end after each write.
     */
    private fun applyTextIfChanged(incoming: String) {
        val et = view ?: return
        val current = et.text?.toString() ?: ""
        if (current == incoming) return
        programmaticWrite = true
        try {
            et.setText(incoming)
            // Move cursor to end after an external value change.
            et.setSelection(et.text?.length ?: 0)
        } finally {
            programmaticWrite = false
        }
    }

    /**
     * Build and dispatch a `{ detail: { value: "<text>" } }` custom
     * event matching the shape [`InputEvent`] on the Rust side
     * deserializes. The `detail` wrapper is the Lynx custom-event
     * convention.
     *
     * For `focus` and `blur` the [text] is empty and the Rust handler
     * ignores it (the event fires no value), but we keep the same
     * payload shape for consistency.
     *
     * ## Deferred dispatch (reentrancy guard)
     *
     * The actual `WhiskerCustomEvent.dispatch` is deferred to the next
     * main-loop tick via `View.post { ... }`. A synchronous dispatch
     * inside a UI callback can reenter Rust while Rust holds the
     * renderer borrow during a hot-reload remount / teardown — e.g.
     * Lynx `remove_child` triggers a native focus-loss callback that
     * fires `blur` / `change` synchronously, reentering
     * `dispatch_event` and panicking with "RefCell already borrowed"
     * (the event is dropped). Posting breaks the synchronous reentry.
     *
     * The suppression decision is made BEFORE we reach here: the
     * caller (`afterTextChanged`) reads `programmaticWrite` and the
     * text synchronously, so deferring only the dispatch doesn't
     * weaken the `input`-echo guard. The two-way `input` round-trip
     * arriving one tick late is absorbed by the cursor-diff guard in
     * [applyTextIfChanged]. The `name` + `text` (hence `params`) are
     * captured synchronously into the posted lambda.
     */
    private fun emitEvent(name: String, text: String) {
        // Pass the payload directly as the params — do NOT wrap it in a
        // `detail` key. The Android event reporter (WhiskerView's
        // LynxEventReporter) already places the dispatched params UNDER a
        // `detail` key in the event body (matching iOS's
        // `generateEventBody`: `{ type, target, currentTarget, detail }`).
        // The Rust `InputEvent { detail: { value } }` reads `body.detail`.
        // Wrapping here too would double-nest and the value would never
        // reach `on_input` (it would always deliver an empty string).
        val params = mapOf("value" to text)
        // `post` runs on the next UI-loop iteration on the main thread.
        // No-op if the view is detached (post returns false; there's
        // nothing to dispatch into anyway).
        view?.post {
            WhiskerCustomEvent.dispatch(
                ui = this,
                name = name,
                params = params,
            )
        }
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
        /// Lynx splits the CSS `border-radius` shorthand into these four
        /// per-corner keys in `mBackingMap`. Same list WhiskerImageView
        /// reads; each value is a `[px, unit, px, unit]` PlatformLength
        /// quartet with the density-multiplied px at index 0.
        val CORNER_KEYS = listOf(
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        )
    }
}
