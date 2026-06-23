#!/usr/bin/env python3
# SPDX-License-Identifier: MulanPSL-2.0
"""Zero-copy RGB camera consumer — standalone example.

Subscribes via shared memory and prints frame metadata.  Does NOT require
Atlas or any Robonix framework — only rclpy + robonix_zc.

Usage::

    # Start the provider first, then:
    python zc_camera_consumer.py
"""

from __future__ import annotations

import logging
import time

import rclpy
from rclpy.node import Node
from sensor_msgs.msg import Image

from robonix_zc import ZcSubscriber, init_shm, shutdown_shm

log = logging.getLogger("zc_camera_consumer")

SHM_NAME = "robonix_zc"
SHM_SIZE = 67_108_864
TOPIC    = "/camera/rgb_zc"

_frame_count = 0
_last_print = time.time()


def on_frame(img: Image) -> None:
    """Callback — log frame info periodically."""
    global _frame_count, _last_print
    _frame_count += 1
    now = time.time()
    if now - _last_print >= 2.0:
        fps = _frame_count / (now - _last_print)
        size_mb = len(img.data) / (1024 * 1024)
        log.info(
            "frame %d: %dx%d %s  %.1f MB  %.1f fps",
            _frame_count,
            img.width,
            img.height,
            img.encoding,
            size_mb,
            fps,
        )
        _frame_count = 0
        _last_print = now


def main() -> None:
    rclpy.init()
    node = Node("zc_camera_consumer")

    init_shm(SHM_NAME, SHM_SIZE)
    log.info("shared memory ready: %s", SHM_NAME)

    sub = ZcSubscriber(
        node=node,
        topic_name=TOPIC,
        msg_type=Image,
        callback=on_frame,
        qos_profile="best_effort",
    )
    log.info("subscribed to %s (ZC)", TOPIC)

    try:
        rclpy.spin(node)
    except KeyboardInterrupt:
        pass
    finally:
        sub.close()
        shutdown_shm(SHM_NAME)
        node.destroy_node()
        rclpy.shutdown()


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)-5s %(name)s: %(message)s")
    main()
