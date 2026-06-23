# SPDX-License-Identifier: MulanPSL-2.0
"""Python zero-copy transport for ROS 2 heavy-payload messages.

Thin wrapper over ``_zc_bridge`` (pybind11 → librobonix_zc.so). Provides
two classes for the common patterns:

* ``ZcPublisher``  — write Image/PointCloud2 into shared memory, publish
                      only the object name over a ROS ``String`` topic.
* ``ZcSubscriber`` — subscribe to the ``String`` topic, look up the object
                      in shared memory, unpack into a standard ROS message,
                      and invoke a user callback.

Usage::

    from robonix_zc import ZcPublisher, ZcSubscriber, init_shm, shutdown_shm

    init_shm("robonix_zc", 67108864)

    pub = ZcPublisher(node, "/camera/zc", Image)
    pub.publish(image_msg)

    sub = ZcSubscriber(node, "/camera/zc", Image, my_callback)
"""

from robonix_zc.zc_publisher import ZcPublisher
from robonix_zc.zc_subscriber import ZcSubscriber

from . import _zc_bridge as bridge


def init_shm(name: str = "robonix_zc", size: int = 67108864) -> None:
    """Initialise (or open) the shared-memory segment.

    Must be called once before any publisher or subscriber.  Multiple
    processes that share the same *name* will share the same segment.
    """
    bridge.init_shm(name, size)


def shutdown_shm(name: str = "robonix_zc") -> None:
    """Release this process's reference to the shared-memory segment.

    The last process to call ``shutdown_shm`` cleans up the segment.
    """
    bridge.shutdown_shm(name)
