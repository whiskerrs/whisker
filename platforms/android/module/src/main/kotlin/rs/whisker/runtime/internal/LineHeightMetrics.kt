package rs.whisker.runtime.internal

public fun centeredLineAscent(ascent: Int, descent: Int, targetHeight: Int): Int {
    val adjustment = targetHeight - (descent - ascent)
    return ascent - adjustment / 2
}

public fun centeredLineDescent(ascent: Int, descent: Int, targetHeight: Int): Int {
    val adjustment = targetHeight - (descent - ascent)
    return descent + adjustment - adjustment / 2
}
