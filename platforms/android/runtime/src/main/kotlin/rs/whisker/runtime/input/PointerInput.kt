package rs.whisker.runtime.input

import android.view.MotionEvent

/** One Android pointer sample normalized to Whisker's logical surface contract. */
internal data class HostPointerInput(
    val timestampMs: Double,
    val event: Int,
    val pointerId: Long,
    val kind: Int,
    val x: Float,
    val y: Float,
    val buttons: Int,
    val changedButton: Int,
)

/** Projects one Android event into the per-pointer events consumed by the Rust runtime. */
internal fun normalizePointerInput(event: MotionEvent, density: Float): List<HostPointerInput> {
    if (!density.isFinite() || density <= 0f) return emptyList()
    val eventKind = when (event.actionMasked) {
        MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> POINTER_DOWN
        MotionEvent.ACTION_MOVE,
        MotionEvent.ACTION_HOVER_ENTER,
        MotionEvent.ACTION_HOVER_MOVE,
        MotionEvent.ACTION_HOVER_EXIT,
        MotionEvent.ACTION_SCROLL,
        -> POINTER_MOVE
        MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP -> POINTER_UP
        MotionEvent.ACTION_CANCEL -> POINTER_CANCEL
        else -> return emptyList()
    }
    val indices = when (event.actionMasked) {
        MotionEvent.ACTION_MOVE,
        MotionEvent.ACTION_HOVER_ENTER,
        MotionEvent.ACTION_HOVER_MOVE,
        MotionEvent.ACTION_HOVER_EXIT,
        MotionEvent.ACTION_SCROLL,
        MotionEvent.ACTION_CANCEL,
        ->
            0 until event.pointerCount
        else -> event.actionIndex..event.actionIndex
    }
    return indices.map { index ->
        val pointerKind = pointerKind(event.getToolType(index))
        HostPointerInput(
            timestampMs = event.eventTime.toDouble(),
            event = eventKind,
            pointerId = event.getPointerId(index).toLong() + POINTER_ID_OFFSET,
            kind = pointerKind,
            x = event.getX(index) / density,
            y = event.getY(index) / density,
            buttons = normalizedButtons(event, eventKind, pointerKind),
            changedButton = changedButton(event, eventKind, pointerKind),
        )
    }
}

private fun pointerKind(toolType: Int): Int = when (toolType) {
    MotionEvent.TOOL_TYPE_MOUSE -> POINTER_MOUSE
    MotionEvent.TOOL_TYPE_FINGER -> POINTER_TOUCH
    MotionEvent.TOOL_TYPE_STYLUS, MotionEvent.TOOL_TYPE_ERASER -> POINTER_PEN
    else -> POINTER_UNKNOWN
}

private fun normalizedButtons(event: MotionEvent, eventKind: Int, pointerKind: Int): Int =
    if (pointerKind == POINTER_TOUCH || pointerKind == POINTER_PEN) {
        if (eventKind == POINTER_UP || eventKind == POINTER_CANCEL) 0 else PRIMARY_BUTTON_MASK
    } else {
        event.buttonState
    }

private fun changedButton(event: MotionEvent, eventKind: Int, pointerKind: Int): Int {
    if (pointerKind == POINTER_TOUCH) return NO_CHANGED_BUTTON
    if (eventKind != POINTER_DOWN && eventKind != POINTER_UP) return NO_CHANGED_BUTTON
    val button = event.actionButton.takeIf { it != 0 }
        ?: event.buttonState.takeIf { it != 0 }
        ?: MotionEvent.BUTTON_PRIMARY
    return when (button) {
        MotionEvent.BUTTON_PRIMARY -> 0
        MotionEvent.BUTTON_TERTIARY -> 1
        MotionEvent.BUTTON_SECONDARY -> 2
        MotionEvent.BUTTON_BACK -> 3
        MotionEvent.BUTTON_FORWARD -> 4
        else -> NO_CHANGED_BUTTON
    }
}

private const val POINTER_ID_OFFSET = 1L
private const val POINTER_DOWN = 0
private const val POINTER_MOVE = 1
private const val POINTER_UP = 2
private const val POINTER_CANCEL = 3
private const val POINTER_MOUSE = 0
private const val POINTER_TOUCH = 1
private const val POINTER_PEN = 2
private const val POINTER_UNKNOWN = 3
private const val PRIMARY_BUTTON_MASK = 1
private const val NO_CHANGED_BUTTON = -1
