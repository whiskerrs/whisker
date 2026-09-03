package rs.whisker.gradle

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class ProcessCaptureTest {
    @Test
    fun drainsLargeStderrWithoutBlockingStdout() {
        val shell = File("/bin/sh")
        if (!shell.isFile) return
        val process = ProcessBuilder(
            shell.absolutePath,
            "-c",
            "yes x | head -c 262144 >&2; printf stderr-tail >&2; printf module-report",
        ).start()

        val result = captureProcess(process)

        assertEquals(0, result.exitCode)
        assertEquals("module-report", result.stdout)
        assertTrue(result.stderr.endsWith("stderr-tail"))
        assertTrue(result.stderr.length <= 64 * 1024)
    }
}
