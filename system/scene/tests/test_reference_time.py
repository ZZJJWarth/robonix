# SPDX-License-Identifier: MulanPSL-2.0
"""Reference-time freshness and generation changes without a simulator."""

import asyncio
from types import SimpleNamespace

import pytest
import robonix_chronos_clock as native

from scene_service import clock
from scene_service.ingest.ros_subscribers import _LatestSlot
from scene_service.state.object_registry import BBox3D, ObjectRegistry, Pose3D


@pytest.fixture
def reference():
    previous = clock._reference_generation
    clock._reference_generation = None
    native.configure_reference("ros_time")

    def update(seconds, generation=1, available=True):
        native.update_reference_time(int(seconds * 1_000_000_000), generation, available)

    update(0)
    try:
        yield update
    finally:
        native.configure_reference("linux_system")
        clock._reference_generation = previous


def test_zero_and_pause_keep_observations_fresh(reference, monkeypatch):
    slot = _LatestSlot()
    slot.write("frame")
    registry = ObjectRegistry(grace_period_s=5)
    obj = registry.insert_object("cup", Pose3D(1, 2, 0), BBox3D(.1, .1, .1), .9, clock.time())
    monkeypatch.setattr(clock, "time", lambda: obj.last_seen + 3600)
    assert slot.read()[0] == "frame"
    assert slot.age() == obj.age() == 0
    assert registry.mark_stale(clock.time()) == 0
    reference(4)
    assert registry.mark_stale(clock.time()) == 0
    reference(6)
    assert registry.mark_stale(clock.time()) == 1
    assert registry.prune_expired(clock.time(), ttl_s=5) == [obj.object_id]


def test_reset_hides_old_samples_and_preserves_calibration(reference):
    slot = _LatestSlot()
    calibration = _LatestSlot(reference_sensitive=False)
    slot.write("frame")
    calibration.write("intrinsics")
    registry = ObjectRegistry()
    registry.insert_object("cup", Pose3D(1, 2, 0), BBox3D(.1, .1, .1), .9, clock.time())
    reference(0, generation=2)
    assert slot.read()[0] is None
    assert asyncio.run(registry.snapshot()) == {}
    slot.clear()
    calibration.clear()
    assert calibration.read()[0] == "intrinsics"
    slot.write("new frame")
    assert slot.read()[0] == "new frame"
    assert slot.reference_receipt() == (0, 2)


def test_unavailable_reference_never_uses_wall_time(reference):
    slot = _LatestSlot()
    slot.write("frame")
    received = slot.read()
    reference(0, available=False)
    with pytest.raises(OSError):
        clock.reference_time()
    assert slot.read()[0] is None
    slot.write("unavailable frame")
    reference(0)
    assert slot.read() == received
    assert clock.time() > 1_000_000_000


def test_pose_retains_selected_receipt_after_new_delivery(reference):
    from scene_service.robot_geometry import RobotFootprint, RobotGeometryState
    from scene_service.service import _SelfTracker

    async def exercise():
        registry = ObjectRegistry()
        geometry = RobotGeometryState()
        geometry.update(RobotFootprint(((0, 0), (1, 0), (0, 1)), "base_link", .1, 1))
        tracker = _SelfTracker(registry, geometry)
        tracker.world_frame_id = "map"
        slot = _LatestSlot()
        selected = object()
        slot.write(selected)
        receipt = slot.reference_receipt(selected)
        reference(3)
        slot.write(object())
        await tracker.on_pose(1, 2, 0, .1, receipt=receipt)
        robot = registry.get_object(tracker._object_id)
        assert robot._reference_receipt == (0, 1)
        assert robot.age() == 3
        reference(0, generation=2)
        await tracker.on_pose(9, 9, 0, .5, receipt=receipt)
        assert robot.pose.x == 1

    asyncio.run(exercise())


def test_vlm_sampling_follows_reference_and_stop_remains_real(reference):
    from scene_service.ingest.perception_vlm import VLMObjectDetector

    detector = VLMObjectDetector(rgb_fetcher=lambda: None, camera_to_world_fn=lambda: None,
                                 on_detections=None, period_s=.02)
    detector.api_key = "test-only"
    calls = []

    async def tick():
        calls.append(True)

    detector._tick = tick

    async def exercise():
        await detector.start()
        try:
            await asyncio.sleep(.1)
            assert len(calls) == 1
            reference(.01)
            await asyncio.sleep(.06)
            assert len(calls) == 1
            reference(.02)
            await asyncio.sleep(.06)
            assert len(calls) == 2
        finally:
            await asyncio.wait_for(detector.stop(), timeout=.2)

    asyncio.run(exercise())


@pytest.mark.parametrize("clock_reset", [False, True])
def test_vlm_drops_inference_completed_after_reset(reference, monkeypatch, clock_reset):
    from scene_service.ingest import perception_vlm as vlm

    detector = vlm.VLMObjectDetector(rgb_fetcher=lambda: b"jpeg", camera_to_world_fn=lambda: None,
                                     on_detections=None)
    monkeypatch.setattr(vlm, "fingerprint_jpeg", lambda _: None)
    published = []

    async def infer(_):
        if clock_reset:
            reference(0, generation=2)
        else:
            await detector.reset_derived_state()
        return [{"label": "cup"}]

    async def publish(detections):
        published.extend(detections)

    detector._call_vlm = infer
    detector._publish_detections = publish
    asyncio.run(detector._tick())
    assert published == []
    assert detector._last_success_detections is None


def test_linux_transition_is_unavailable_until_reset_finishes(monkeypatch):
    from scene_service.reference_time import subscription

    reading = SimpleNamespace(timestamp_ns=10, generation=1, available=True, reason="")
    monkeypatch.setattr(native, "reference_time", lambda: reading)
    previous = clock._reference_generation

    async def exercise():
        entered, release = asyncio.Event(), asyncio.Event()
        failures = []

        async def reset():
            entered.set()
            await release.wait()

        async with subscription({"kind": "linux_system"}, "scene-test", failures.append) as callbacks:
            callbacks.append(reset)
            reading.generation = 2
            await asyncio.wait_for(entered.wait(), timeout=1)
            with pytest.raises(OSError):
                clock.reference_time()
            release.set()
            for _ in range(100):
                if clock._reference_generation == 2:
                    break
                await asyncio.sleep(.01)
            assert clock.reference_time().generation == 2
        assert not failures

    try:
        asyncio.run(exercise())
    finally:
        clock._reference_generation = previous
        native.configure_reference("linux_system")


def test_concept_graphs_does_not_repeat_a_frozen_rgb_delivery(reference, monkeypatch):
    import sys
    import numpy as np
    from scene_service.ingest import perception_concept_graphs as cg

    slot = _LatestSlot()
    slot.write(object())
    detector = cg.ConceptGraphsDetector(
        rgb_fetcher_msg=lambda: slot.read()[0],
        depth_fetcher_msg=lambda: object(),
        camera_info_fetcher=lambda: None,
        on_detections=None,
        registry=ObjectRegistry(),
        hub=SimpleNamespace(reference_receipt=lambda _kind, msg: slot.reference_receipt(msg)),
    )
    calls = []

    def predict(*args, **kwargs):
        calls.append(True)
        return [SimpleNamespace(boxes=[])]

    detector._yolo = SimpleNamespace(predict=predict)
    detector._cam_info = lambda: SimpleNamespace(fx=1, fy=1)
    monkeypatch.setitem(sys.modules, "torch", SimpleNamespace())
    monkeypatch.setattr(cg, "_image_msg_to_bgr", lambda _msg: np.zeros((2, 2, 3), dtype=np.uint8))
    monkeypatch.setattr(cg, "_depth_msg_to_metres", lambda _msg: np.ones((2, 2)))
    detector._tick_once()
    detector._tick_once()
    assert len(calls) == 1
    assert detector._tick_idx == 1
    slot.write(object())
    detector._tick_once()
    assert len(calls) == 1
    reference(detector._period_s)
    detector._tick_once()
    assert len(calls) == 2


def test_graph_debounce_requires_reference_progress_and_new_evidence(reference, tmp_path, monkeypatch):
    from scene_service.scene_graph.geometric_loop import GeometricRelationLoop
    from scene_service.scene_graph.store import SceneGraphStore

    registry = ObjectRegistry()
    objects = [registry.insert_object(label, Pose3D(x, 0, 1), BBox3D(.1, .1, .1),
                                      .9, clock.time())
               for label, x in (("cup", 0), ("table", .5))]
    for obj in objects:
        obj.observation_count = 3
    store = SceneGraphStore(tmp_path)
    loop = GeometricRelationLoop(registry, store, hz=2)

    async def exercise():
        await loop._tick()
        for _ in range(10):
            await loop._tick()
        assert store.get_geometric_edges() == []
        reference(10)
        unrelated = registry.insert_object("chair", Pose3D(10, 0, 1), BBox3D(.1, .1, .1),
                                            .9, clock.time())
        unrelated.observation_count = 3
        await loop._tick()
        assert store.get_geometric_edges() == []
        objects[0].observation_count += 1
        reference(11)
        await loop._tick()
        assert len(store.get_geometric_edges()) == 2
        original_snapshot = registry.snapshot

        async def reset_during_snapshot():
            objects = await original_snapshot()
            store.clear_derived_state()
            return objects

        monkeypatch.setattr(registry, "snapshot", reset_during_snapshot)
        await loop._tick()
        assert store.get_snapshot() is None
        monkeypatch.setattr(registry, "snapshot", original_snapshot)
        reference(0, generation=2)
        store.clear_derived_state()
        await loop._tick()
        assert store.get_snapshot() is None

    asyncio.run(exercise())


def test_semantic_rounds_do_not_age_repeated_evidence(reference, tmp_path):
    from scene_service.scene_graph.builder import SceneGraphBuilder, SceneGraphConfig
    from scene_service.scene_graph.store import SceneGraphStore
    from scene_service.scene_graph.types import SceneGraphEdge

    registry = ObjectRegistry()
    objects = [registry.insert_object(label, Pose3D(x, 0, 1), BBox3D(.1, .1, .1),
                                      .9, clock.time())
               for label, x in (("cup", 0), ("table", .5))]
    for obj in objects:
        obj.observation_count = 3
    store = SceneGraphStore(tmp_path)
    store.set_semantic_edges([SceneGraphEdge(objects[0].object_id, objects[1].object_id,
                                            "on_top_of", .9)])
    cfg = SceneGraphConfig()
    cfg.relation_enabled = cfg.image_relations_enabled = False
    builder = SceneGraphBuilder(registry, SimpleNamespace(), store, cfg)

    async def exercise():
        await builder.rebuild_once()
        assert store.get_semantic_edges()[0].stale_rounds == 1
        for _ in range(10):
            await builder.rebuild_once()
        reference(100)
        unrelated = registry.insert_object("chair", Pose3D(10, 0, 1), BBox3D(.1, .1, .1),
                                            .9, clock.time())
        unrelated.observation_count = 3
        await builder.rebuild_once()
        assert store.get_semantic_edges()[0].stale_rounds == 1
        objects[0].observation_count += 1
        reference(150)
        await builder.rebuild_once()
        assert store.get_semantic_edges()[0].stale_rounds == 2
        reference(200)
        objects[0].observation_count += 1
        await builder.rebuild_once()
        assert store.get_semantic_edges() == []

    asyncio.run(exercise())


@pytest.mark.parametrize("clock_reset", [False, True])
def test_pending_semantic_inference_cannot_restore_reset_cache(reference, tmp_path, clock_reset):
    from scene_service.scene_graph.builder import SceneGraphBuilder, SceneGraphConfig
    from scene_service.scene_graph.store import SceneGraphStore
    from scene_service.scene_graph.types import SceneGraphEdge

    registry = ObjectRegistry()
    for x in (0, .5):
        obj = registry.insert_object("cup", Pose3D(x, 0, 1), BBox3D(.1, .1, .1),
                                     .9, clock.time())
        obj.observation_count = 3
    store = SceneGraphStore(tmp_path)
    cfg = SceneGraphConfig()
    cfg.image_relations_enabled = False

    async def infer(a, b, hint):
        if clock_reset:
            reference(0, generation=2)
        store.clear_derived_state()
        return SceneGraphEdge(a.object_id, b.object_id, "on_top_of", .9)

    builder = SceneGraphBuilder(registry, SimpleNamespace(infer_relation=infer), store, cfg)

    async def exercise():
        with pytest.raises(OSError):
            await builder.rebuild_once()
        assert store.get_semantic_edges() == []
        assert store._relation_cache == {}

    asyncio.run(exercise())
