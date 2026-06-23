# SPDX-License-Identifier: MulanPSL-2.0
"""ZcPublisher: write a ROS message into shared memory, then publish only
the object name over a ROS 2 ``String`` topic."""

from __future__ import annotations

from typing import Any

from rclpy.node import Node
from sensor_msgs.msg import Image, PointCloud2, PointField
from std_msgs.msg import String

from . import _zc_bridge as bridge


def _normalized_topic(topic_name: str) -> str:
    """Strip leading '/' so the shm-side topic key matches."""
    return topic_name[1:] if topic_name.startswith("/") else topic_name


def _image_to_object_name(msg: Image) -> str:
    return bridge.create_image(
        msg.header.stamp.sec,
        msg.header.stamp.nanosec,
        msg.header.frame_id,
        msg.width,
        msg.height,
        msg.encoding,
        int(msg.is_bigendian),
        msg.step,
        memoryview(msg.data),
    )


def _pc2_fields_to_dicts(fields: list[PointField]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for field in fields:
        out.append(
            {
                "name": field.name,
                "offset": int(field.offset),
                "datatype": int(field.datatype),
                "count": int(field.count),
            }
        )
    return out


def _pointcloud2_to_object_name(msg: PointCloud2) -> str:
    return bridge.create_pointcloud2(
        msg.header.stamp.sec,
        msg.header.stamp.nanosec,
        msg.header.frame_id,
        msg.height,
        msg.width,
        _pc2_fields_to_dicts(list(msg.fields)),
        int(msg.is_bigendian),
        msg.point_step,
        msg.row_step,
        int(msg.is_dense),
        memoryview(msg.data),
    )


class ZcPublisher:
    """Publish ``Image`` or ``PointCloud2`` via shared memory.

    The publisher writes the full message into shared memory and then
    publishes only the object name (``"ShmObject_<id>"``) on a ROS 2
    ``std_msgs/String`` topic.  Subscribers read the data directly from
    the shared-memory segment.
    """

    def __init__(
        self,
        node: Node,
        topic_name: str,
        msg_type: type[Any],
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
        self._pub = node.create_publisher(String, topic_name, qos_profile)

    def publish(self, msg: Any) -> bool:
        """Write *msg* into shared memory and publish the object name.

        Returns ``False`` when there are no subscribers (message is
        discarded without touching the ROS topic).
        """
        if self._msg_type is Image:
            object_name = _image_to_object_name(msg)
        else:
            object_name = _pointcloud2_to_object_name(msg)

        published = bridge.prepare_publish(self._shm_topic_name, object_name)
        if not published:
            return False

        out = String()
        out.data = object_name
        self._pub.publish(out)
        return True
