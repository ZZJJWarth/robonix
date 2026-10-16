# SPDX-License-Identifier: MulanPSL-2.0
"""Opt-in /clock -> Chronos -> Soma smoke test with real ROS and gRPC processes.

Build Atlas, Chronos and Soma, install the clock binding, source ROS, then run
with Python providing grpcio, grpcio-tools and PyYAML. No simulator is required.
"""

from pathlib import Path
import json
import os
import signal
import subprocess
import sys
import tempfile
import threading
import time

import grpc
import grpc_tools
import rclpy
from nav_msgs.msg import Odometry
from rosgraph_msgs.msg import Clock

from rbnx_boot_smoke import port, wait


def run():
    root = Path(__file__).resolve().parents[3]
    artifacts = Path(tempfile.mkdtemp(prefix="robonix-simulation-reference-"))
    print(f"Artifacts: {artifacts}", flush=True)
    stubs = artifacts / "stubs"
    stubs.mkdir()
    generated = max(root.glob("target/debug/build/robonix-soma-*/out/robonix_contracts.proto"),
                    key=lambda path: path.stat().st_mtime_ns).parent
    subprocess.run([sys.executable, "-m", "grpc_tools.protoc", f"-I{generated}",
                    f"-I{Path(grpc_tools.__file__).parent / '_proto'}",
                    f"--python_out={stubs}", f"--grpc_python_out={stubs}",
                    *map(str, generated.glob("*.proto"))], check=True)
    subprocess.run([sys.executable, "-m", "grpc_tools.protoc", f"-I{root / 'system/atlas/proto'}",
                    f"--python_out={stubs}", f"--grpc_python_out={stubs}",
                    str(root / "system/atlas/proto/atlas.proto")], check=True)
    sys.path.insert(0, str(stubs))
    import atlas_pb2 as a
    import atlas_pb2_grpc as ag
    import robonix_contracts_pb2_grpc as contracts
    import soma_pb2 as soma
    from google.protobuf.empty_pb2 import Empty

    environment = {**os.environ, "ROS_DOMAIN_ID": str(100 + os.getpid() % 100),
                   "SCRIBE_LOG_DIR": str(artifacts / "logs")}
    os.environ["ROS_DOMAIN_ID"] = environment["ROS_DOMAIN_ID"]
    atlas_address, chronos_address, soma_address = [f"127.0.0.1:{port()}" for _ in range(3)]
    reference_config = {"core": {"reference": {"kind": "ros_time"},
                                 "quality": {"refresh_interval_ns": 10_000_000_000}},
                        "reference_reader_command": [sys.executable, "-u", "{script}"]}
    manifest = artifacts / "robonix_manifest.yaml"
    manifest.write_text(json.dumps({"manifestVersion": 1, "name": "simulation-reference-test",
                                   "system": {"chronos": reference_config}}))
    robot_yaml = artifacts / "soma.yaml"
    robot_yaml.write_text(json.dumps({"robot": {"id": "reference-test"}, "urdf": {"path": "robot.urdf"}}))
    (artifacts / "robot.urdf").write_text('<robot name="reference-test"><link name="base_link"/></robot>')
    children, logs, channels, streams = [], [], [], []
    node = None
    cache, lock = {}, threading.Lock()

    def spawn(binary, args):
        log = (artifacts / f"{binary}.log").open("w")
        logs.append(log)
        child = subprocess.Popen([str(root / "target/debug" / binary), *args], env=environment,
                                 cwd=artifacts, stdout=log, stderr=log)
        children.append(child)
        return child

    def connect(address):
        channel = grpc.insecure_channel(address)
        channels.append(channel)
        grpc.channel_ready_future(channel).result(timeout=10)
        return channel

    def current_reference():
        with lock:
            return cache.get("reference")

    try:
        spawn("robonix-atlas", ["--listen", atlas_address, "--capabilities", str(root / "capabilities")])
        atlas = ag.AtlasStub(connect(atlas_address))
        atlas.RegisterPrimitive(a.RegisterRequest(id="reference-chassis", namespace="robonix/primitive/chassis"), timeout=2)
        for contract, transport, endpoint in [
            ("robonix/lifecycle/driver", a.TRANSPORT_GRPC, "127.0.0.1:1"),
            ("robonix/primitive/chassis/odom", a.TRANSPORT_ROS2, "/reference_test_odom"),
        ]:
            params = a.TransportParams(grpc=a.GrpcParams(
                proto_file="capabilities/lifecycle/driver.v1.toml",
                service_name="robonix.contracts.RobonixLifecycleDriver",
                method="/robonix.contracts.RobonixLifecycleDriver/Driver",
            )) if transport == a.TRANSPORT_GRPC else a.TransportParams(ros2=a.Ros2Params(qos_profile="reliable"))
            atlas.DeclareCapability(a.DeclareCapabilityRequest(provider_id="reference-chassis", contract_id=contract,
                                    transport=transport, endpoint=endpoint, params=params), timeout=2)
        for state in (a.STATE_INACTIVE, a.STATE_ACTIVE):
            atlas.SetLifecycleState(a.SetLifecycleStateRequest(id="reference-chassis", state=state), timeout=2)
        chronos = spawn("robonix-chronos", ["--atlas", atlas_address, "--listen", chronos_address,
                                           "--config-json", json.dumps(reference_config)])
        reference_stub = contracts.RobonixSystemChronosReferenceTimeStub(connect(chronos_address))
        wait(lambda: any(provider.state == a.STATE_ACTIVE for provider in
                        atlas.Query(a.QueryRequest(id="chronos"), timeout=2).providers))
        stream = reference_stub.ReferenceTime(Empty())
        streams.append(stream)

        def consume():
            try:
                for reading in stream:
                    with lock:
                        cache["reference"] = reading
            except grpc.RpcError:
                pass

        threading.Thread(target=consume, daemon=True).start()
        wait(lambda: current_reference() is not None)
        assert not current_reference().available, "missing /clock must not fall back to wall time"
        rclpy.init(args=[])
        node = rclpy.create_node("robonix_reference_smoke")
        clock_pub = node.create_publisher(Clock, "/clock", 1)
        odom_pub = node.create_publisher(Odometry, "/reference_test_odom", 10)
        wait(lambda: clock_pub.get_subscription_count() > 0)

        def publish_clock(seconds):
            message = Clock()
            message.clock.sec = seconds
            clock_pub.publish(message)
            wait(lambda: current_reference().available and current_reference().timestamp_ns == seconds * 1_000_000_000)

        publish_clock(0)
        initial_generation = current_reference().generation
        soma_process = spawn("robonix-soma", ["--atlas", atlas_address, "--listen", soma_address,
                             "--robot-yaml", str(robot_yaml), "--deployment-manifest", str(manifest),
                             "--config-json", json.dumps({"runtime_reader_command": [sys.executable, "-u", "{script}", "{config}"]})])
        health = contracts.RobonixSystemSomaGetHealthStub(connect(soma_address))
        wait(lambda: odom_pub.get_subscription_count() > 0)

        def chassis():
            snapshot = health.GetHealth(soma.GetHealth_Request(), timeout=2).snapshot
            return next((component for component in snapshot.components if component.name == "reference-chassis"), None)

        odom = Odometry()
        odom.twist.twist.linear.x = 0.2
        odom_pub.publish(odom)
        wait(lambda: chassis() is not None and chassis().health == 0)
        before = health.GetHealth(soma.GetHealth_Request(), timeout=2).snapshot.soma_ts_ns
        time.sleep(6.0)
        assert current_reference().timestamp_ns == 0
        assert current_reference().generation == initial_generation
        assert chassis().health == 0, "paused reference must stay fresh despite a ten-second quality cadence"
        after = health.GetHealth(soma.GetHealth_Request(), timeout=2).snapshot.soma_ts_ns
        assert after > before, "wall publication timestamps must keep advancing during a pause"
        node.destroy_publisher(clock_pub)
        wait(lambda: not current_reference().available)
        clock_pub = node.create_publisher(Clock, "/clock", 1)
        wait(lambda: clock_pub.get_subscription_count() > 0)
        time.sleep(1.0)
        assert not current_reference().available, "publisher presence must not reuse its old reading"
        publish_clock(0)
        assert current_reference().generation != initial_generation
        initial_generation = current_reference().generation
        wait(lambda: chassis() is None)
        odom_pub.publish(odom)
        wait(lambda: chassis() is not None and chassis().health == 0)
        publish_clock(1)
        time.sleep(0.7)
        assert chassis().health == 0, "one simulated second must not expire a two-second sample"
        publish_clock(3)
        wait(lambda: chassis().health == 3)
        assert current_reference().generation == initial_generation, "forward steps must not reset generation"
        publish_clock(0)
        assert current_reference().generation != initial_generation
        wait(lambda: chassis() is None)
        odom_pub.publish(odom)
        wait(lambda: chassis() is not None and chassis().health == 0)
        chronos.send_signal(signal.SIGTERM)
        chronos.wait(timeout=10)
        wait(lambda: "reference stream" in (artifacts / "robonix-soma.log").read_text())
        time.sleep(0.7)
        assert soma_process.poll() is None, "Soma must retain its real process lifecycle after reference loss"
        print("PASS: zero, pause, publisher loss/recovery, slow advance, expiry, reset, wall timestamps and reference loss", flush=True)
    finally:
        for stream in streams:
            stream.cancel()
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=5)
        if node is not None:
            node.destroy_node()
            rclpy.shutdown()
        for channel in channels:
            channel.close()
        for log in logs:
            log.close()


if __name__ == "__main__":
    run()
