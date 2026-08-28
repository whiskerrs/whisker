package rs.whisker.runtime.scene

import rs.whisker.runtime.WhiskerChildPolicy
import rs.whisker.runtime.WhiskerCommandBinding
import rs.whisker.runtime.WhiskerElementRegistration
import rs.whisker.runtime.WhiskerElementRegistry
import rs.whisker.runtime.WhiskerEventBinding
import rs.whisker.runtime.WhiskerMeasurement
import rs.whisker.runtime.WhiskerPropertyBinding
import rs.whisker.runtime.WhiskerValueKind

/** Builds and atomically binds the Rust-provided element schema table. */
internal class HostElementBootstrap {
    private val registrations = ArrayList<WhiskerElementRegistration>()

    fun begin() {
        registrations.clear()
    }

    @Suppress("LongParameterList")
    fun register(
        elementType: Int,
        name: String,
        childPolicy: Int,
        measurement: Int,
        textStyle: Int,
        propertyIds: IntArray,
        propertyKinds: IntArray,
        propertyNames: Array<String>,
        eventIds: IntArray,
        eventKinds: IntArray,
        eventNames: Array<String>,
        commandIds: IntArray,
        commandKinds: IntArray,
        commandNames: Array<String>,
    ) {
        val kinds = WhiskerValueKind.entries
        registrations += WhiskerElementRegistration(
            elementType,
            name,
            WhiskerChildPolicy.entries[childPolicy],
            WhiskerMeasurement.entries[measurement],
            textStyle != 0,
            propertyIds.indices.map {
                WhiskerPropertyBinding(propertyIds[it], propertyNames[it], kinds[propertyKinds[it]])
            },
            eventIds.indices.map {
                WhiskerEventBinding(
                    eventIds[it],
                    eventNames[it],
                    if (eventKinds[it] < 0) null else kinds[eventKinds[it]],
                )
            },
            commandIds.indices.map {
                WhiskerCommandBinding(commandIds[it], commandNames[it], kinds[commandKinds[it]])
            },
        )
    }

    fun finish(): Boolean = WhiskerElementRegistry.bind(registrations)
}
