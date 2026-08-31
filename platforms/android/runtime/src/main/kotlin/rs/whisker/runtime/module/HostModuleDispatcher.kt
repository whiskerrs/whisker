package rs.whisker.runtime.module

import rs.whisker.runtime.WhiskerModuleEventCenter
import rs.whisker.runtime.WhiskerModuleRegistry
import rs.whisker.runtime.WhiskerValue

/** Bridges typed module invocations to the Host-local module registry. */
internal class HostModuleDispatcher(
    private val resolve: (Long, Long, WhiskerValue) -> Unit,
) {
    fun invoke(
        module: String,
        method: String,
        args: Array<WhiskerValue>,
        isAsync: Boolean,
        callbackPtr: Long,
        userDataPtr: Long,
    ): Boolean = try {
        val settle: (WhiskerValue) -> Unit = { value ->
            resolve(callbackPtr, userDataPtr, value)
        }
        if (isAsync && WhiskerModuleRegistry.invokeDispatchAsync(module, method, args, settle)) {
            true
        } else {
            settle(WhiskerModuleRegistry.invokeDispatch(module, method, args))
            true
        }
    } catch (error: Throwable) {
        resolve(
            callbackPtr,
            userDataPtr,
            WhiskerValue.Err(
                "module $module.$method failed: ${error.message ?: error.javaClass.simpleName}",
            ),
        )
        true
    }

    fun observe(owner: Any, module: String, event: String, observing: Boolean) {
        WhiskerModuleEventCenter.setObserving(owner, module, event, observing)
    }
}
