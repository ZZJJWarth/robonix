# SPDX-License-Identifier: MulanPSL-2.0

import asyncio
import contextlib
import ctypes
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

from scene_service import clock


class ClockTests(unittest.TestCase):
    def test_reads_keep_unix_and_monotonic_units(self):
        before = time.time()
        reference_ns = clock.time_ns()
        reference_s = clock.time()
        after = time.time()
        self.assertIsInstance(reference_ns, int)
        self.assertIsInstance(reference_s, float)
        self.assertLessEqual(before, reference_ns / 1_000_000_000.0)
        self.assertLessEqual(reference_ns / 1_000_000_000.0, reference_s)
        self.assertLessEqual(reference_s, after)
        before = time.monotonic()
        steady = clock.monotonic()
        self.assertLessEqual(before, steady)
        self.assertLessEqual(steady, time.monotonic())

    @unittest.skipUnless(sys.platform == "linux", "requires Linux clock interposition")
    def test_native_clock_failures_and_reference_steps(self):
        scene = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory) / "clock_failure.so"
            fixture = scene.parent / "atlas/tests/clock_failure.c"
            subprocess.run([
                "cc", "-shared", "-fPIC", "-std=c11", "-Wall", "-Wextra", "-Werror",
                str(fixture), "-ldl", "-o", str(library),
            ], check=True)
            result = subprocess.run([
                sys.executable, str(Path(__file__).resolve()), "--fault-case",
            ], env={**os.environ, "LD_PRELOAD": str(library),
                    "PYTHONPATH": str(scene) + os.pathsep + os.environ.get("PYTHONPATH", "")},
                capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


def fault_case():
    from scene_service.annotations import AnnotationStore
    from scene_service.ingest.ros_subscribers import _LatestSlot
    from scene_service.map_meta import make_meta
    from scene_service.scene_graph.store import SceneGraphStore
    from scene_service.scene_graph.types import SceneGraphEdge, SceneGraphNode
    from scene_service.state.object_registry import BBox3D, ObjectRegistry, Pose3D, now_unix

    mode = ctypes.CDLL(None).robonix_test_clock_mode
    mode.argtypes = [ctypes.c_int]
    mode.restype = None
    case = unittest.TestCase()
    slot = _LatestSlot()
    slot.write("first")
    received = slot.read()
    with tempfile.TemporaryDirectory() as directory:
        annotations = AnnotationStore(directory, map_id="map")
        ann = annotations.create(kind="region", name="desk", points=[[0, 0], [1, 0], [0, 1]])
        saved = annotations.path.read_bytes()
        graph = SceneGraphStore(Path(directory) / "graph")
        nodes = [SceneGraphNode("cup", "cup", (1, 2, 3), (.1, .1, .1))]
        edges = [SceneGraphEdge("cup", "desk", "on_top_of")]
        graph.set_geometric(nodes, edges)
        graph.set_semantic_edges(edges)
        snapshot = graph.get_snapshot()
        for failure in [1, 2, 4]:
            mode(failure)
            with case.assertRaises(OSError):
                clock.time_ns()
            with case.assertRaises(OSError):
                clock.time()
            case.assertGreater(clock.monotonic(), 0)
            slot.write("failed")
            case.assertEqual(slot.read(), received)
            with case.assertRaises(OSError):
                annotations.update(ann.annotation_id, name="changed")
            case.assertEqual(ann.name, "desk")
            case.assertEqual(annotations.path.read_bytes(), saved)
            with case.assertRaises(OSError):
                graph.set_geometric([], [])
            with case.assertRaises(OSError):
                graph.set_semantic_edges([])
            case.assertEqual(graph.get_snapshot(), snapshot)
        mode(0)
        slot.write("recovered")
        case.assertEqual(slot.read()[0], "recovered")
        case.assertEqual(slot.read()[2], received[2] + 1)
        ann = annotations.update(ann.annotation_id, name="changed")
        case.assertEqual(ann.name, "changed")
        case.assertIsInstance(json.loads(annotations.path.read_text())["annotations"][0]["updated_at"], float)
        case.assertGreater(make_meta("map", "map__s1", 1).saved_at_unix, 1_000_000_000)

    now = now_unix()
    registry = ObjectRegistry(grace_period_s=5.0)
    obj = registry.insert_object("cup", Pose3D(1, 2, 0), BBox3D(.1, .1, .1), .9, now)
    steady = clock.monotonic()
    mode(32)
    case.assertLess(now_unix(), now - 86000)
    case.assertEqual(registry.mark_stale(now_unix()), 0)
    case.assertEqual(registry.prune_expired(now_unix(), ttl_s=30.0), [])
    mode(8)
    case.assertGreater(now_unix(), now + 86000)
    case.assertEqual(registry.mark_stale(now_unix()), 1)
    case.assertEqual(registry.prune_expired(now_unix(), ttl_s=30.0), [obj.object_id])
    case.assertLess(clock.monotonic() - steady, 2.0)
    mode(16)
    with case.assertRaises(OSError):
        clock.monotonic()
    case.assertGreaterEqual(clock.monotonic(), steady)
    mode(0)

    async def stale_loop_recovers():
        from scene_service.service import _stale_tick

        registry = ObjectRegistry(grace_period_s=0.01)
        obj = registry.insert_object("cup", Pose3D(1, 2, 0), BBox3D(.1, .1, .1), .9, now_unix() - 1)
        mode(1)
        task = asyncio.create_task(_stale_tick(registry, period_s=.01))
        try:
            await asyncio.sleep(.03)
            case.assertFalse(task.done())
            case.assertFalse(obj.missing)
            mode(0)
            await asyncio.sleep(.03)
            case.assertFalse(task.done())
            case.assertTrue(obj.missing)
        finally:
            mode(0)
            task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await task

    asyncio.run(stale_loop_recovers())

    async def tracker_recovers():
        from scene_service.robot_geometry import RobotFootprint, RobotGeometryState
        from scene_service.service import _SelfTracker

        registry = ObjectRegistry()
        geometry = RobotGeometryState()
        geometry.update(RobotFootprint(((0, 0), (1, 0), (0, 1)), "base_link", .1, 1))
        tracker = _SelfTracker(registry, geometry)
        tracker.world_frame_id = "map"
        await tracker.on_pose(1, 2, 0, .1)
        robot = registry.get_object(tracker._object_id)
        before = (robot.pose, robot.bbox, robot.last_seen, tracker.latest_xy_yaw())
        mode(1)
        try:
            with case.assertRaises(OSError):
                await tracker.on_pose(3, 4, 0, .5)
            case.assertEqual(
                (robot.pose, robot.bbox, robot.last_seen, tracker.latest_xy_yaw()), before
            )
        finally:
            mode(0)
        await tracker.on_pose(3, 4, 0, .5)
        case.assertEqual(robot.pose.yaw, .5)
        case.assertEqual(robot.bbox.yaw, .5)
        case.assertEqual(tracker.latest_xy_yaw(), (3, 4, 0, .5))

    asyncio.run(tracker_recovers())


if __name__ == "__main__":
    if sys.argv[1:] == ["--fault-case"]:
        fault_case()
    else:
        unittest.main()
