package rs.whisker.runtime.bridge

import android.os.Build

/** Immutable renderer profile advertised once for each mounted Whisker surface. */
internal data class HostRenderProfile(
    val abiMajor: Int,
    val abiMinor: Int,
    val protocolMajor: Int,
    val protocolMinor: Int,
    val native: Long,
    val emulated: Long,
) {
    fun wireValues(): LongArray = longArrayOf(
        abiMajor.toLong(),
        abiMinor.toLong(),
        protocolMajor.toLong(),
        protocolMinor.toLong(),
        native,
        emulated,
    )
}

internal object AndroidHostCapabilities {
    private const val COMMON_NATIVE =
        MobileAbi.CAPABILITY_ELLIPTICAL_BORDER_RADIUS or
            MobileAbi.CAPABILITY_VISUAL_EFFECTS or
            MobileAbi.CAPABILITY_TEXT_EFFECTS or
            MobileAbi.CAPABILITY_TEXT_TYPOGRAPHY or
            MobileAbi.CAPABILITY_CURSOR or
            MobileAbi.CAPABILITY_RESOURCE_LIFECYCLE or
            MobileAbi.CAPABILITY_LINEAR_GRADIENTS or
            MobileAbi.CAPABILITY_RADIAL_GRADIENTS or
            MobileAbi.CAPABILITY_CONIC_GRADIENTS or
            MobileAbi.CAPABILITY_BACKGROUND_GEOMETRY or
            MobileAbi.CAPABILITY_BACKGROUND_LAYER_STACKING or
            MobileAbi.CAPABILITY_BACKGROUND_IMAGE_RESOURCES or
            MobileAbi.CAPABILITY_RADIAL_GRADIENT_VARIANTS

    fun current(): HostRenderProfile = forApiLevel(Build.VERSION.SDK_INT)

    fun forApiLevel(apiLevel: Int): HostRenderProfile = HostRenderProfile(
        abiMajor = MobileAbi.MOBILE_ABI_MAJOR,
        abiMinor = MobileAbi.MOBILE_ABI_MINOR,
        protocolMajor = MobileAbi.FRAME_PROTOCOL_MAJOR,
        protocolMinor = MobileAbi.FRAME_PROTOCOL_MINOR,
        native = COMMON_NATIVE or if (apiLevel >= 31) {
            MobileAbi.CAPABILITY_BACKDROP_BLUR
        } else {
            0
        },
        emulated = 0,
    )
}
