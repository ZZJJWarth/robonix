# SPDX-License-Identifier: MulanPSL-2.0

import contextlib
import ctypes
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import Mock, patch


HELPER = Path(__file__).resolve().parents[1] / "src/runtime_ros.py"
spec = importlib.util.spec_from_file_location("runtime_ros", HELPER)
reader = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reader)


class ReaderTests(unittest.TestCase):
    def test_missing_binding_reports_startup_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / "config.json"
            config.write_text('{"sources": []}', encoding="utf-8")
            output = io.StringIO()
            with patch.dict(sys.modules, {"robonix_chronos_clock": None}), \
                    patch.object(sys, "argv", [str(HELPER), str(config)]), \
                    contextlib.redirect_stdout(output):
                self.assertEqual(reader.main(), 2)
            event = json.loads(output.getvalue())
            self.assertEqual(event["kind"], "warning")
            self.assertIn("install robonix-chronos-clock", event["message"])

    @unittest.skipUnless(sys.platform == "linux", "requires Linux clock interposition")
    def test_callbacks_skip_failed_reads_and_recover(self):
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory) / "clock_failure.so"
            fixture = HELPER.parents[2] / "atlas/tests/clock_failure.c"
            subprocess.run([
                "cc", "-shared", "-fPIC", "-std=c11", "-Wall", "-Wextra", "-Werror",
                str(fixture), "-ldl", "-o", str(library),
            ], check=True)
            result = subprocess.run([
                sys.executable, str(Path(__file__).resolve()), "--fault-case",
            ], env={**os.environ, "LD_PRELOAD": str(library)}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


def fault_case():
    import robonix_chronos_clock as clock

    mode = ctypes.CDLL(None).robonix_test_clock_mode
    mode.argtypes = [ctypes.c_int]
    mode.restype = None
    callbacks = []
    node = Mock()
    node.create_subscription.side_effect = lambda _, topic, callback, qos: callbacks.append(callback)
    ros = ModuleType("rclpy")
    ros.init = Mock()
    ros.create_node = Mock(return_value=node)
    ros.shutdown = Mock()
    qos = ModuleType("rclpy.qos")
    qos.QoSProfile = lambda depth: SimpleNamespace(depth=depth)
    qos.ReliabilityPolicy = SimpleNamespace(BEST_EFFORT=1)
    nav = ModuleType("nav_msgs.msg")
    nav.Odometry = object
    sensor = ModuleType("sensor_msgs.msg")
    sensor.JointState = object
    joint = SimpleNamespace(name=["gripper"], position=[0.08])
    vector = SimpleNamespace(x=0.2, y=0.0, z=0.0)
    odom = SimpleNamespace(twist=SimpleNamespace(twist=SimpleNamespace(linear=vector, angular=vector)))
    output = io.StringIO()
    case = unittest.TestCase()

    def spin(_, timeout_sec):
        for callback, msg, expected in zip(callbacks, [joint, odom], [
            {"kind": "joint_state", "provider_id": "arm", "names": ["gripper"], "positions": [0.08]},
            {"kind": "odom", "provider_id": "base", "linear": [0.2, 0.0, 0.0], "angular": [0.2, 0.0, 0.0]},
        ]):
            before = clock.reference_timestamp_ns() / 1_000_000_000.0
            callback(msg)
            event = json.loads(output.getvalue().splitlines()[-1])
            case.assertEqual({key: value for key, value in event.items() if key != "received_at_unix"}, expected)
            case.assertIsInstance(event["received_at_unix"], float)
            case.assertGreaterEqual(event["received_at_unix"], before)
            case.assertLessEqual(event["received_at_unix"], clock.reference_timestamp_ns() / 1_000_000_000.0)
            for failure in [1, 2, 4]:
                count = len(output.getvalue().splitlines())
                mode(failure)
                callback(msg)
                events = output.getvalue().splitlines()
                case.assertEqual(len(events), count + 1)
                case.assertEqual(json.loads(events[-1])["kind"], "warning")
            mode(0)
            callback(msg)
            recovered = json.loads(output.getvalue().splitlines()[-1])
            case.assertEqual({key: value for key, value in recovered.items() if key != "received_at_unix"}, expected)

    ros.spin_once = spin
    with tempfile.TemporaryDirectory() as directory:
        config = Path(directory) / "config.json"
        config.write_text(json.dumps({"sources": [
            {"kind": "joint_state", "provider_id": "arm", "topic": "/joints"},
            {"kind": "odom", "provider_id": "base", "topic": "/odom"},
        ]}), encoding="utf-8")
        with patch.dict(sys.modules, {"rclpy": ros, "rclpy.qos": qos,
                                      "nav_msgs.msg": nav, "sensor_msgs.msg": sensor}), \
                patch.object(sys, "argv", [str(HELPER), str(config)]), \
                contextlib.redirect_stdout(output), \
                patch.object(reader.select, "select", side_effect=[([], [], []), ([sys.stdin], [], [])]), \
                patch.object(reader.os, "read", return_value=b""):
            case.assertEqual(reader.main(), 0)
    case.assertEqual(json.loads(output.getvalue().splitlines()[0]), {"kind": "ready", "subscriptions": 2})
    node.destroy_node.assert_called_once()
    ros.shutdown.assert_called_once()


if __name__ == "__main__":
    if sys.argv[1:] == ["--fault-case"]:
        fault_case()
    else:
        unittest.main()
