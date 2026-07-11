# SPDX-License-Identifier: MulanPSL-2.0
"""Python lifecycle helpers for Robonix zero-copy shared memory.

Usage::

    from robonix_zc import init_shm, shutdown_shm

    init_shm("robonix_zc", 67108864)
"""

from . import _zc_bridge as native


def init_shm(name: str = "robonix_zc", size: int = 67108864) -> None:
    """Initialise (or open) the shared-memory segment.

    Multiple processes that share the same *name* will share the same segment.
    """
    native.init_shm(name, size)


def shutdown_shm(name: str = "robonix_zc") -> None:
    """Release this process's reference to the shared-memory segment.

    The last process to call ``shutdown_shm`` cleans up the segment.
    """
    native.shutdown_shm(name)
