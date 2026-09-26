package rs.whisker.modules.firebasecrashlytics

import com.google.firebase.crashlytics.FirebaseCrashlytics
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerValue

/** A Rust error report; Crashlytics groups non-fatals by the exception class and stack. */
class RustError(name: String, reason: String) : Exception("$name: $reason")

class RustPanic(message: String) : RuntimeException(message)

@WhiskerModule
class FirebaseCrashlyticsModule : Module() {
    private val crashlytics get() = FirebaseCrashlytics.getInstance()

    override fun definition() = ModuleDefinition {
        Name("FirebaseCrashlytics")
        Function("log") { args -> attempt { crashlytics.log(requireNotNull(args.firstOrNull()?.asString())) } }
        Function("setUserId") { args -> attempt { crashlytics.setUserId(args.firstOrNull()?.asString() ?: "") } }
        Function("setCustomKey") { args ->
            attempt {
                val key = requireNotNull(args.firstOrNull()?.asString())
                when (val value = args.getOrNull(1)) {
                    is WhiskerValue.Str -> crashlytics.setCustomKey(key, value.value)
                    is WhiskerValue.Int -> crashlytics.setCustomKey(key, value.value)
                    is WhiskerValue.Float -> crashlytics.setCustomKey(key, value.value)
                    is WhiskerValue.Bool -> crashlytics.setCustomKey(key, value.value)
                    else -> throw IllegalArgumentException("Unsupported custom key value")
                }
            }
        }
        Function("recordError") { args ->
            attempt {
                val report = requireNotNull(args.firstOrNull() as? WhiskerValue.Map).value
                val error = RustError(
                    requireNotNull(report["name"]?.asString()),
                    requireNotNull(report["reason"]?.asString()),
                )
                val frames = stackTrace(report)
                if (frames.isNotEmpty()) error.stackTrace = frames
                crashlytics.recordException(error)
            }
        }
        Function("recordPanic") { args ->
            // recordException writes on a background executor that the abort would cut short;
            // the uncaught-exception handler records synchronously and ends the process.
            val report = (args.firstOrNull() as? WhiskerValue.Map)?.value.orEmpty()
            val panic = RustPanic(report["reason"]?.asString() ?: "Rust panic")
            val frames = stackTrace(report)
            if (frames.isNotEmpty()) panic.stackTrace = frames
            val thread = Thread.currentThread()
            thread.uncaughtExceptionHandler?.uncaughtException(thread, panic)
            WhiskerValue.Map(mapOf("value" to WhiskerValue.Null))
        }
        Function("isCollectionEnabled") { _ ->
            try { success(WhiskerValue.Bool(crashlytics.isCrashlyticsCollectionEnabled)) } catch (error: Exception) { failure(error) }
        }
        Function("setCollectionEnabled") { args ->
            attempt { crashlytics.isCrashlyticsCollectionEnabled = requireNotNull(args.firstOrNull()?.asBool()) }
        }
        AsyncFunction("checkForUnsentReports") { _, promise ->
            try {
                crashlytics.checkForUnsentReports().addOnCompleteListener { task ->
                    promise.resolve(if (task.isSuccessful) success(WhiskerValue.Bool(task.result == true)) else failure(task.exception))
                }
            } catch (error: Exception) { promise.resolve(failure(error)) }
        }
        Function("sendUnsentReports") { _ -> attempt { crashlytics.sendUnsentReports() } }
        Function("deleteUnsentReports") { _ -> attempt { crashlytics.deleteUnsentReports() } }
        Function("didCrashOnPreviousExecution") { _ ->
            try { success(WhiskerValue.Bool(crashlytics.didCrashOnPreviousExecution())) } catch (error: Exception) { failure(error) }
        }
        Function("crash") { _ ->
            // Module calls may catch exceptions, so hand the crash straight to the uncaught
            // exception handler that Crashlytics installs.
            val thread = Thread.currentThread()
            thread.uncaughtExceptionHandler?.uncaughtException(thread, RuntimeException("Whisker Firebase Crashlytics test crash"))
            WhiskerValue.Null
        }
    }

    private fun stackTrace(report: Map<String, WhiskerValue>): Array<StackTraceElement> =
        (report["frames"] as? WhiskerValue.Array)?.value.orEmpty().mapNotNull { frame ->
            val fields = (frame as? WhiskerValue.Map)?.value ?: return@mapNotNull null
            val symbol = fields["symbol"]?.asString() ?: return@mapNotNull null
            val owner = symbol.substringBeforeLast("::", "rust")
            StackTraceElement(owner, symbol.substringAfterLast("::"), fields["file"]?.asString(), fields["line"]?.asInt()?.toInt() ?: -1)
        }.toTypedArray()

    private fun attempt(body: () -> Unit): WhiskerValue =
        try { body(); success(WhiskerValue.Null) } catch (error: Exception) { failure(error) }

    private fun success(value: WhiskerValue) = WhiskerValue.Map(mapOf("value" to value))
    private fun failure(error: Exception?): WhiskerValue {
        val code = when (error) {
            is IllegalArgumentException -> "invalid-argument"
            is IllegalStateException -> "app-not-configured"
            else -> "unknown"
        }
        return WhiskerValue.Map(mapOf("error" to WhiskerValue.Map(mapOf(
            "code" to WhiskerValue.Str(code),
            "message" to WhiskerValue.Str(error?.message ?: "Crashlytics operation failed"),
        ))))
    }
}
