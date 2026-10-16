# SPDX-License-Identifier: MulanPSL-2.0
"""Wall timestamps, real steady time, and the selected robot reference."""

import logging

import robonix_chronos_clock

log = logging.getLogger(__name__)
_reference_generation = None


def time_ns() -> int:
    try:
        return robonix_chronos_clock.wall_timestamp_ns()
    except OSError:
        log.warning("[scene-clock] wall time read failed", exc_info=True)
        raise


def time() -> float:
    return time_ns() / 1_000_000_000.0


def monotonic() -> float:
    try:
        return robonix_chronos_clock.steady_time().timestamp_ns / 1_000_000_000.0
    except OSError:
        log.warning("[scene-clock] steady time read failed", exc_info=True)
        raise


def reference_time():
    reading = robonix_chronos_clock.reference_time()
    if not reading.available or reading.timestamp_ns < 0:
        raise OSError(f"Chronos reference unavailable: {reading.reason}")
    if _reference_generation is not None and reading.generation != _reference_generation:
        raise OSError("Chronos reference generation reset is pending")
    return reading


def reference_seconds() -> float:
    return reference_time().timestamp_ns / 1_000_000_000.0


def reference_age(timestamp_ns: int, generation: int) -> float:
    reading = reference_time()
    if reading.generation != generation or reading.timestamp_ns < timestamp_ns:
        return float("inf")
    return (reading.timestamp_ns - timestamp_ns) / 1_000_000_000.0


class _ReferenceBudget:
    def __init__(self):
        self.start = None

    def elapsed(self) -> float:
        try:
            reading = reference_time()
        except OSError:
            self.start = None
            return 0.0
        if self.start is None or reading.generation != self.start.generation:
            self.start = reading
        if reading.timestamp_ns < self.start.timestamp_ns:
            raise OSError("Chronos reference moved backwards within a generation")
        return (reading.timestamp_ns - self.start.timestamp_ns) / 1e9


class _ReferenceSampling:
    def __init__(self):
        self.previous = None

    def admit(self, reading, period_s: float) -> bool:
        previous = self.previous
        if previous is not None and previous.generation == reading.generation:
            elapsed = (reading.timestamp_ns - previous.timestamp_ns) / 1e9
            if elapsed < 0:
                raise OSError("Chronos reference moved backwards within a generation")
            if elapsed <= 0 or elapsed < period_s:
                return False
        self.previous = reading
        return True
