# SPDX-License-Identifier: MulanPSL-2.0
"""Chronos reads in Scene's existing seconds and nanoseconds units."""

import logging

import robonix_chronos_clock

log = logging.getLogger(__name__)


def time_ns() -> int:
    try:
        return robonix_chronos_clock.reference_timestamp_ns()
    except OSError:
        log.warning("[scene-clock] reference time read failed", exc_info=True)
        raise


def time() -> float:
    return time_ns() / 1_000_000_000.0


def monotonic() -> float:
    try:
        return robonix_chronos_clock.steady_time().timestamp_ns / 1_000_000_000.0
    except OSError:
        log.warning("[scene-clock] steady time read failed", exc_info=True)
        raise
