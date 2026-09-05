import contextlib
import io
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

import release_with_retry as release


# Actual cargo diagnostic from the failed v0.13.3 release (run 33906332276).
THROTTLED = (
    "the remote server responded with an error (status 429 Too Many Requests): "
    "You have published too many updates to existing crates in a short period "
    "of time. Please try again after Fri, 04 Sep 2026 18:40:23 GMT and see "
    "https://crates.io/docs/rate-limits for more details."
)
RESET = 1788547223


class RetryTests(unittest.TestCase):
    def test_server_reset_time_including_slack(self):
        self.assertEqual(release.retry_delay(THROTTLED, RESET - 38), 48)

    def test_duplicate_cargo_diagnostics_do_not_double_wait(self):
        self.assertEqual(release.retry_delay(THROTTLED + "\n" + THROTTLED, RESET - 38), 48)

    def test_stale_missing_or_invalid_time_uses_fallback(self):
        for diagnostic, now in [
            (THROTTLED, RESET + 1),
            (THROTTLED.replace("Fri, 04 Sep 2026 18:40:23 GMT", "invalid GMT"), RESET),
            (THROTTLED.replace("Please try again after Fri, 04 Sep 2026 18:40:23 GMT", ""), RESET),
        ]:
            with self.subTest(diagnostic=diagnostic):
                self.assertEqual(release.retry_delay(diagnostic, now), 120)

    def test_other_errors_are_not_retryable(self):
        for diagnostic in [
            "error[E0432]: unresolved import",
            "registry at https://crates.io returned status 403 Forbidden",
            "GitHub returned status 429 Too Many Requests",
            "published crate version 0.4.29; compilation failed",
        ]:
            with self.subTest(diagnostic=diagnostic):
                self.assertIsNone(release.retry_delay(diagnostic, RESET))

    def run_attempts(self, results):
        with (
            patch.object(release, "run_release", side_effect=results) as run,
            patch.object(release.time, "sleep") as sleep,
            patch.object(release.time, "time", return_value=RESET - 38),
            contextlib.redirect_stdout(io.StringIO()),
            contextlib.redirect_stderr(io.StringIO()),
        ):
            status = release.release_with_retry(["release-plz", "release"])
        return status, run, sleep

    def test_success_does_not_retry(self):
        status, run, sleep = self.run_attempts([(0, "already published")])
        self.assertEqual(status, 0)
        self.assertEqual(run.call_count, 1)
        sleep.assert_not_called()

    def test_repeated_throttling_resumes_until_success(self):
        status, run, sleep = self.run_attempts([(1, THROTTLED), (1, THROTTLED), (0, "published")])
        self.assertEqual(status, 0)
        self.assertEqual(run.call_count, 3)
        self.assertEqual([call.args for call in sleep.call_args_list], [(48,), (48,)])

    def test_error_after_throttling_stops_using_only_current_output(self):
        status, run, sleep = self.run_attempts([(1, THROTTLED), (101, "compilation failed")])
        self.assertEqual(status, 101)
        self.assertEqual(run.call_count, 2)
        sleep.assert_called_once()

    def test_retry_budget_exhaustion_is_failure(self):
        status, run, sleep = self.run_attempts([(1, THROTTLED)] * release.MAX_ATTEMPTS)
        self.assertEqual(status, 1)
        self.assertEqual(run.call_count, release.MAX_ATTEMPTS)
        self.assertEqual(sleep.call_count, release.MAX_ATTEMPTS - 1)

    def test_signal_termination_is_failure_without_retry(self):
        status, run, sleep = self.run_attempts([(-15, "")])
        self.assertEqual(status, 1)
        self.assertEqual(run.call_count, 1)
        sleep.assert_not_called()

    def test_real_subprocess_resumes_partial_release_and_streams_both_outputs(self):
        # A fake publisher persists the first published crate before throttling.
        # The second invocation must retain cwd/env/state and finish the remainder.
        with tempfile.TemporaryDirectory() as directory:
            publisher = Path(directory) / "publisher.py"
            publisher.write_text(
                "import os, pathlib, sys\n"
                "state = pathlib.Path(os.environ['RETRY_TEST_STATE'])\n"
                "if not state.exists():\n"
                "    state.write_text(os.getcwd())\n"
                "    print('published crate-a', flush=True)\n"
                f"    print({THROTTLED!r}, file=sys.stderr)\n"
                "    sys.exit(1)\n"
                "assert state.read_text() == os.getcwd()\n"
                "print('crate-a: already published; published crate-b')\n"
            )
            output = io.StringIO()
            with (
                patch.dict(os.environ, {"RETRY_TEST_STATE": str(Path(directory) / "state")}),
                patch.object(release.time, "sleep") as sleep,
                patch.object(release.time, "time", return_value=RESET - 38),
                contextlib.redirect_stdout(output),
            ):
                status = release.release_with_retry([sys.executable, str(publisher)])
            self.assertEqual(status, 0)
            sleep.assert_called_once_with(48)
            self.assertIn(THROTTLED, output.getvalue())
            self.assertIn("crate-a: already published; published crate-b", output.getvalue())


if __name__ == "__main__":
    unittest.main()
