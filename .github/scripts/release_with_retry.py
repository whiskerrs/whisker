"""Resume release-plz after crates.io's publish rate limit, on the same checkout."""

import math
import os
import re
import subprocess
import sys
import time
from email.utils import parsedate_to_datetime


MAX_ATTEMPTS = 40
FALLBACK_DELAY_SECONDS = 120
RATE_LIMIT = re.compile(
    r"status 429 Too Many Requests[^\n]*https://crates\.io/docs/rate-limits"
)
RETRY_AT = re.compile(r"Please try again after (.+? GMT)")


def retry_delay(output, now):
    """Only retry crates.io publish throttling; honor its advertised reset time."""
    responses = RATE_LIMIT.findall(output)
    if not responses:
        return None
    delays = []
    for response in responses:
        match = RETRY_AT.search(response)
        try:
            reset = parsedate_to_datetime(match[1]).timestamp() if match else None
        except (TypeError, ValueError, OverflowError):
            reset = None
        # Give the server a little slack for clock skew / rounding. If the
        # advertised time is stale or absent, avoid a tight retry loop.
        delays.append(
            max(10, math.ceil(reset - now) + 10)
            if reset is not None and reset > now
            else FALLBACK_DELAY_SECONDS
        )
    return max(delays)


def run_release(command):
    # Stream diagnostics to Actions while retaining this attempt's output for
    # classification. Never log command arguments: they contain the GitHub token.
    with subprocess.Popen(
        command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, errors="replace", bufsize=1,
    ) as process:
        output = []
        for line in process.stdout:
            print(line, end="", flush=True)
            output.append(line)
        return process.wait(), "".join(output)


def release_with_retry(command):
    for attempt in range(1, MAX_ATTEMPTS + 1):
        print(f"release-plz attempt {attempt}/{MAX_ATTEMPTS}", flush=True)
        status, output = run_release(command)
        if status == 0:
            return 0
        delay = retry_delay(output, time.time())
        if delay is None or attempt == MAX_ATTEMPTS:
            print("Release failed; no further automatic retries.", file=sys.stderr)
            return status if status > 0 else 1
        print(
            f"crates.io rate limit: waiting {delay}s before resuming unpublished crates.",
            flush=True,
        )
        time.sleep(delay)
    return 1


if __name__ == "__main__":
    sys.exit(release_with_retry([
        "release-plz", "release", "--git-token", os.environ["GITHUB_TOKEN"],
    ]))
