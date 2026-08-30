package rs.whisker.runtime

import org.junit.Assert.assertEquals
import org.junit.Test

class WhiskerModuleEventCenterTest {
    @Test
    fun moduleEventsReachOnlyObservingSurfacesAndLifecycleIsAggregated() {
        val starts = mutableListOf<String>()
        val stops = mutableListOf<String>()
        val module = object : Module() {
            override fun definition(): ModuleDefinition = ModuleDefinition {
                Name("Clock")
                Events("tick")
                OnStartObserving("tick") { starts += "start" }
                OnStopObserving("tick") { stops += "stop" }
            }
        }.also {
            it.qualifiedName = "event-test:Clock"
            WhiskerModuleEventCenter.register(it)
        }
        val moduleName = requireNotNull(module.qualifiedName)
        val firstOwner = Any()
        val secondOwner = Any()
        val received = mutableListOf<String>()
        var firstPayload: WhiskerValue? = null
        WhiskerModuleEventCenter.installEventSink(firstOwner) { _, _, payload ->
            received += "first"
            firstPayload = payload
        }
        WhiskerModuleEventCenter.installEventSink(secondOwner) { _, _, _ -> received += "second" }

        try {
            WhiskerModuleEventCenter.setObserving(firstOwner, moduleName, "tick", true)
            assertEquals(listOf("start"), starts)
            val mutableBytes = byteArrayOf(1, 2, 3)
            WhiskerModuleEventCenter.dispatchSend(
                moduleName,
                "tick",
                WhiskerValue.Bytes(mutableBytes),
            )
            mutableBytes[0] = 9
            assertEquals(listOf("first"), received)
            assertEquals(WhiskerValue.Bytes(byteArrayOf(1, 2, 3)), firstPayload)

            received.clear()
            WhiskerModuleEventCenter.setObserving(secondOwner, moduleName, "tick", true)
            assertEquals(listOf("start"), starts)
            WhiskerModuleEventCenter.dispatchSend(moduleName, "tick", WhiskerValue.Int(2))
            assertEquals(setOf("first", "second"), received.toSet())

            WhiskerModuleEventCenter.installEventSink(firstOwner, null)
            assertEquals(emptyList<String>(), stops)
            received.clear()
            WhiskerModuleEventCenter.dispatchSend(moduleName, "tick", WhiskerValue.Int(3))
            assertEquals(listOf("second"), received)
            WhiskerModuleEventCenter.installEventSink(secondOwner, null)
            assertEquals(listOf("stop"), stops)
        } finally {
            WhiskerModuleEventCenter.installEventSink(firstOwner, null)
            WhiskerModuleEventCenter.installEventSink(secondOwner, null)
        }
    }
}
