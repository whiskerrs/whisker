package rs.whisker.runtime.resource

/** Android projection of the typed mobile resource-channel constants. */
internal object HostResourceAbi {
    const val COMMAND_LOAD = 1
    const val COMMAND_RELEASE = 2

    const val KIND_RASTER_IMAGE = 1

    const val SOURCE_NONE = 0
    const val SOURCE_URL = 1
    const val SOURCE_BUNDLED_ASSET = 2
    const val SOURCE_BYTES = 3

    const val EVENT_READY = 1
    const val EVENT_FAILED = 2

    const val FAILURE_NONE = 0
    const val DIMENSIONS_PRESENT = 1
}

/** Fully owned event payload passed from the Android resource service to JNI. */
internal data class HostResourceAbiEvent(
    val status: Int,
    val failureCode: Int,
    val resourceId: Long,
    val generation: Long,
    val width: Float,
    val height: Float,
    val scale: Float,
    val dimensionsMask: Int,
    val diagnostic: String,
)

/**
 * Typed, non-JSON adapter between the borrowed C ABI command and Host service.
 *
 * JNI has already copied strings and bytes before entering this class. The
 * additional ByteArray copy below transfers ownership to asynchronous Host I/O.
 */
internal class HostResourceChannel(
    private val service: HostResourceService,
) {
    @Suppress("LongParameterList")
    fun accept(
        command: Int,
        kind: Int,
        source: Int,
        resourceId: Long,
        generation: Long,
        identifier: String,
        data: ByteArray,
    ): Boolean = when (command) {
        HostResourceAbi.COMMAND_LOAD -> load(
            kind,
            source,
            resourceId,
            generation,
            identifier,
            data,
        )
        HostResourceAbi.COMMAND_RELEASE -> {
            if (
                kind != 0 || source != HostResourceAbi.SOURCE_NONE ||
                identifier.isNotEmpty() || data.isNotEmpty()
            ) {
                false
            } else {
                service.release(resourceId, generation)
            }
        }
        else -> false
    }

    @Suppress("LongParameterList")
    private fun load(
        kind: Int,
        source: Int,
        resourceId: Long,
        generation: Long,
        identifier: String,
        data: ByteArray,
    ): Boolean {
        if (kind != HostResourceAbi.KIND_RASTER_IMAGE) {
            return service.fail(
                resourceId,
                generation,
                HostResourceFailureCode.Unsupported,
                "Android Host does not support resource kind $kind",
            )
        }
        val rasterSource = when (source) {
            HostResourceAbi.SOURCE_URL -> {
                if (identifier.isBlank() || data.isNotEmpty()) return false
                HostRasterSource.Url(identifier)
            }
            HostResourceAbi.SOURCE_BUNDLED_ASSET -> {
                if (identifier.isBlank() || data.isNotEmpty()) return false
                HostRasterSource.BundledAsset(identifier)
            }
            HostResourceAbi.SOURCE_BYTES -> {
                if (identifier.isBlank() || data.isEmpty()) return false
                HostRasterSource.Bytes(identifier, data.copyOf())
            }
            else -> return false
        }
        return service.load(resourceId, generation, rasterSource)
    }

    companion object {
        fun encodeEvent(snapshot: HostResourceSnapshot): HostResourceAbiEvent? =
            when (snapshot.state) {
                HostResourceState.Ready -> HostResourceAbiEvent(
                    status = HostResourceAbi.EVENT_READY,
                    failureCode = HostResourceAbi.FAILURE_NONE,
                    resourceId = snapshot.resourceId,
                    generation = snapshot.generation,
                    width = snapshot.width.toFloat(),
                    height = snapshot.height.toFloat(),
                    scale = 1f,
                    dimensionsMask = HostResourceAbi.DIMENSIONS_PRESENT,
                    diagnostic = "",
                )
                HostResourceState.Failed -> HostResourceAbiEvent(
                    status = HostResourceAbi.EVENT_FAILED,
                    failureCode = snapshot.failureCode.abiValue,
                    resourceId = snapshot.resourceId,
                    generation = snapshot.generation,
                    width = 0f,
                    height = 0f,
                    scale = 0f,
                    dimensionsMask = 0,
                    diagnostic = snapshot.diagnostic.orEmpty(),
                )
                HostResourceState.Loading,
                HostResourceState.Released,
                -> null
            }
    }
}
