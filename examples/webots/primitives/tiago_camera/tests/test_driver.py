# SPDX-License-Identifier: MulanPSL-2.0
"""Camera initialization stays responsive while simulation has no frames."""

import sys
import threading
from pathlib import Path
from types import ModuleType, SimpleNamespace
from unittest.mock import Mock

import pytest
from robonix_api import Deferred, Err, Ok

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


@pytest.fixture
def driver(monkeypatch):
    from camera_driver import driver

    for package, types in (
        ("geometry_msgs", {"TransformStamped": object}),
        ("sensor_msgs", {"CameraInfo": lambda: SimpleNamespace(header=SimpleNamespace())}),
    ):
        module = ModuleType(package)
        messages = ModuleType(package + ".msg")
        for name, value in types.items():
            setattr(messages, name, value)
        module.msg = messages
        monkeypatch.setitem(sys.modules, package, module)
        monkeypatch.setitem(sys.modules, package + ".msg", messages)
    monkeypatch.setattr(driver, "init_config", None)
    monkeypatch.setattr(driver, "stopping", threading.Event())
    monkeypatch.setattr(driver, "workers", [])
    monkeypatch.setattr(driver, "reference_clock", None)
    monkeypatch.setattr(driver, "latest_rgb_jpeg", None)
    monkeypatch.setattr(driver.tiago_camera, "create_subscription", Mock())
    monkeypatch.setattr(driver.tiago_camera, "create_publisher", Mock(return_value=Mock()))
    monkeypatch.setattr(driver.tiago_camera, "declare_ros2_topic", Mock())
    monkeypatch.setattr(driver, "start_worker", Mock())
    return driver


def test_deferred_init_reuses_subscriptions_and_finishes_on_real_rgb(driver):
    config = {"width": 1, "height": 1, "fx": 1, "fy": 1, "cx": .5, "cy": .5}
    assert isinstance(driver.init(config), Deferred)
    assert isinstance(driver.init(config), Deferred)
    assert driver.tiago_camera.create_subscription.call_count == 3
    assert driver.tiago_camera.create_publisher.call_count == 2
    driver.on_rgb(SimpleNamespace(
        height=1, width=1, encoding="rgb8", data=b"\x00\x00\x00",
        header=SimpleNamespace(stamp=SimpleNamespace(sec=0, nanosec=0), frame_id="camera"),
    ))
    assert isinstance(driver.init(config), Ok)
    assert driver.tiago_camera.create_subscription.call_count == 3
    assert driver.tiago_camera.declare_ros2_topic.call_count == 2


def test_waiting_camera_rejects_changed_config_and_allows_shutdown(driver):
    assert isinstance(driver.init({}), Deferred)
    assert isinstance(driver.init({"rgb_topic": "/different"}), Err)
    assert isinstance(driver.shutdown(), Ok)


def test_snapshot_keeps_the_source_stamp_including_zero(driver):
    assert isinstance(driver.init({}), Deferred)
    driver.on_rgb(SimpleNamespace(
        height=1, width=1, encoding="rgb8", data=b"\x00\x00\x00",
        header=SimpleNamespace(stamp=SimpleNamespace(sec=0, nanosec=7), frame_id="source_camera"),
    ))
    image = driver.snapshot(None)
    assert image.header.stamp.sec == 0
    assert image.header.stamp.nanosec == 7
    assert image.header.frame_id == "source_camera"
