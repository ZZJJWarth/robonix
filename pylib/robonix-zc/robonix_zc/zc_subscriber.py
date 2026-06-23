# SPDX-License-Identifier: MulanPSL-2.0
"""ZcSubscriber: subscribe to a ROS 2 ``String`` topic, look up the
object name in shared memory, unpack into a standard ROS message, and
invoke the user callback."""

from __future__ import annotations

from typing import Any, Callable

from rclpy.node import Node
from sensor_msgs.msg import Image, PointCloud2, PointField
from std_msgs.msg import String

from . import _zc_bridge as bridge


def _normalized_topic(topic_name: str) -> str:
    return topic_name[1:] if topic_name.startswith("/") else topic_name


def _dict_to_image(payload: dict[str, Any]) -> Image:
    msg = Image()
    msg.header.stamp.sec = int(payload["stamp_sec"])
    msg.header.stamp.nanosec = int(payload["stamp_nanosec"])
    msg.header.frame_id = str(payload["frame_id"])
    msg.width = int(payload["width"])
    msg.height = int(payload["height"])
    msg.encoding = str(payload["encoding"])
    msg.is_bigendian = int(payload["is_bigendian"])
    msg.step = int(payload["step"])
    msg.data = payload["data"]
    return msg


def _dict_to_pointcloud2(payload: dict[str, Any]) -> PointCloud2:
    msg = PointCloud2()
    msg.header.stamp.sec = int(payload["stamp_sec"])
    msg.header.stamp.nanosec = int(payload["stamp_nanosec"])
    msg.header.frame_id = str(payload["frame_id"])
    msg.height = int(payload["height"])
    msg.width = int(payload["width"])

    msg.fields = []
    for field in payload["fields"]:
        out_field = PointField()
        out_field.name = str(field["name"])
        out_field.offset = int(field["offset"])
        out_field.datatype = int(field["datatype"])
        out_field.count = int(field["count"])
        msg.fields.append(out_field)

    msg.is_bigendian = int(payload["is_bigendian"])
    msg.point_step = int(payload["point_step"])
    msg.row_step = int(payload["row_step"])
    msg.data = payload["data"]
    msg.is_dense = int(payload["is_dense"])
    return msg


class ZcSubscriber:
    """Subscribe to ``Image`` or ``PointCloud2`` via shared memory.

    The subscriber listens on a ROS 2 ``std_msgs/String`` topic.  When
    an object name arrives it reads the data from shared memory, unpacks
    it into a standard ROS message, and calls *callback*.  The shared-
    memory object is released before the callback returns.
    """

    def __init__(
        self,
        node: Node,
        topic_name: str,
        msg_type: type[Any],
        callback: Callable[[Any], None],
        qos_profile: Any = 10,
    ) -> None:
        if msg_type not in (Image, PointCloud2):
            raise TypeError(
                "msg_type must be sensor_msgs.msg.Image "
                "or sensor_msgs.msg.PointCloud2"
            )

        self._node = node
        self._topic_name = topic_name
        self._shm_topic_name = _normalized_topic(topic_name)
        self._msg_type = msg_type
        self._callback = callback
        self._subscriber_id = bridge.register_subscriber(self._shm_topic_name)

        self._sub = node.create_subscription(
            String, topic_name, self._on_name_msg, qos_profile
        )

    def _on_name_msg(self, msg: String) -> None:
        try:
            if self._msg_type is Image:
                payload = bridge.take_image(msg.data, self._subscriber_id)
                self._callback(_dict_to_image(payload))
            else:
                payload = bridge.take_pointcloud2(msg.data, self._subscriber_id)
                self._callback(_dict_to_pointcloud2(payload))
        except Exception:
            # Don't let a single bad message kill the subscription.
            # The shared-memory object may have already been reclaimed.
            return

    def close(self) -> None:
        """Unregister from the shared-memory topic.

        After ``close()`` this subscriber will no longer receive
        callbacks.  The ROS 2 subscription remains until the node is
        destroyed.
        """
        if getattr(self, "_subscriber_id", 0) != 0:
            bridge.unregister_subscriber(self._shm_topic_name, self._subscriber_id)
            self._subscriber_id = 0

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:
            pass
