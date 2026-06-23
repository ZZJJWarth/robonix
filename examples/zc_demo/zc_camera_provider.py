#!/usr/bin/env python3
# SPDX-License-Identifier: MulanPSL-2.0
"""Zero-copy RGB camera provider — reference implementation.

Owns ``primitive/camera/rgb_zc``.  Publishes images via shared memory:
only an object name travels on the ROS 2 topic; the full Image payload
sits in Boost.Interprocess shared memory.

Usage::

    rbnx boot  # starts atlas + lifecycle orchestration
    python zc_camera_provider.py
"""

from __future__ import annotations

import logging
import threading
import time

from robonix_api import Primitive, Ok
from robonix_api.atlas_types import Transport, Ros2ZcParams
from robonix_api.ros import RosBackend
from robonix_zc import ZcPublisher, init_shm, shutdown_shm
from sensor_msgs.msg import Image

log = logging.getLogger("zc_camera_provider")

SHM_NAME = "robonix_zc"
SHM_SIZE = 67_108_864   # 64 MiB
TOPIC    = "/camera/rgb_zc"
WIDTH    = 1920
HEIGHT  = 1080
FPS      = 30

# ── provider ────────────────────────────────────────────────────────────────

camera = Primitive(
    id="zc_camera_demo",
    namespace="robonix/primitive/camera",
)

# Timer thread for mock frame generation
_timer_stop = threading.Event()
_timer_thread: threading.Thread | None = None


def _gen_frame() -> Image:
    """Generate a mock RGB frame (colour cycles over time)."""
    t = time.time()
    r = int((t * 50) % 256)
    g = int((t * 30) % 256)
    b = int((t * 70) % 256)

    img = Image()
    img.header.stamp.sec = int(t)
    img.header.stamp.nanosec = int((t % 1) * 1e9)
    img.header.frame_id = "camera_zc_frame"
    img.width = WIDTH
    img.height = HEIGHT
    img.encoding = "rgb8"
    img.is_bigendian = 0
    img.step = WIDTH * 3
    img.data = bytes([r, g, b] * (WIDTH * HEIGHT))
    return img


def _publish_loop() -> None:
    """Publish mock frames at FPS until stopped."""
    period = 1.0 / FPS
    pub = ZcPublisher(
        node=RosBackend.get().node,
        topic_name=TOPIC,
        msg_type=Image,
        qos_profile="best_effort",
    )
    log.info("ZC publisher ready on %s", TOPIC)
    while not _timer_stop.wait(period):
        frame = _gen_frame()
        ok = pub.publish(frame)
        if not ok:
            log.debug("no subscribers, frame dropped")


# ── lifecycle ───────────────────────────────────────────────────────────────

@camera.on_init
def init(cfg: dict) -> Ok | str:
    log.info("initialising shared memory: name=%s size=%d", SHM_NAME, SHM_SIZE)
    init_shm(SHM_NAME, SHM_SIZE)

    # Declare the ZC capability to Atlas
    camera.declare_capability(
        contract_id="robonix/primitive/camera/rgb_zc",
        endpoint=TOPIC,
        transport=Transport.ROS2_ZC,
        params=Ros2ZcParams(
            shm_name=SHM_NAME,
            shm_size=SHM_SIZE,
            qos_profile="best_effort",
        ),
        description=f"ZC RGB stream {WIDTH}x{HEIGHT} @ {FPS} Hz",
    )
    log.info("declared rgb_zc capability")

    return Ok()


@camera.on_activate
def activate() -> Ok:
    """Spawn the publishing thread."""
    global _timer_thread
    _timer_stop.clear()
    _timer_thread = threading.Thread(
        target=_publish_loop, name="zc-pub-loop", daemon=True,
    )
    _timer_thread.start()
    log.info("publishing thread started")
    return Ok()


@camera.on_deactivate
def deactivate() -> Ok:
    """Stop publishing."""
    _timer_stop.set()
    log.info("publishing thread stopped")
    return Ok()


@camera.on_shutdown
def shutdown() -> None:
    """Release shared memory."""
    _timer_stop.set()
    shutdown_shm(SHM_NAME)
    log.info("shared memory released")


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)-5s %(name)s: %(message)s")
    camera.run()
