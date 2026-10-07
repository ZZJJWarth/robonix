# SPDX-License-Identifier: MulanPSL-2.0
"""Test-only hardware counter with a real clock bracket and a u32 wrap after 50ms."""
from dataclasses import replace
import time
from chronos_query import capture_query


class Clock:
    """Emulate unsigned microsecond ticks without depending on ROS or an actual vendor SDK."""

    def __init__(self, options):
        self.started = None
        self.mode = options.get("mode", "normal")

    def read(self):
        """Exercise real-time polling, read errors and blocking SDK calls."""
        if self.mode == "error":
            raise RuntimeError("fixture hardware query failed")
        if self.mode == "hang":
            time.sleep(5)
        if self.started is None:
            self.started = time.monotonic_ns()
        sample = capture_query(lambda: (2**32 - 50_000 + (time.monotonic_ns() - self.started) // 1000) % 2**32)
        return replace(sample, uncertainty_ns=2**64 - 1) if self.mode == "overflow" else sample

    def close(self):
        pass


def create(options):
    return Clock(options)
