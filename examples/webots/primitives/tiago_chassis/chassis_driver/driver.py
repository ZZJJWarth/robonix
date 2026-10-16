#!/usr/bin/env python3
# SPDX-License-Identifier: MulanPSL-2.0
# pyright: reportArgumentType=false
"""Tiago chassis primitive — Capability-based driver.

Owns `robonix/primitive/chassis/*`. Publishes /cmd_vel from the `move` rpc.

Capability surface:

  primitive/chassis/driver    rpc        gRPC lifecycle (Capability built-in)
  primitive/chassis/move      rpc        gRPC ExecuteMoveCommand — burst-style
                                          velocity command. Deliberately NOT
                                          MCP — direct LLM control of an
                                          unfiltered velocity primitive bumps
                                          into things. Path-planned motion
                                          goes through service/navigation/*
                                          which composes safe_goal + nav.
  primitive/chassis/odom      topic_out  ROS 2 /odom
  primitive/chassis/twist_in  topic_in   ROS 2 /cmd_vel
"""
from __future__ import annotations

import json
import math
import os
import threading

from robonix_api import Primitive, Ok, Err, Deferred
from robonix_api._reference_time import _ReferenceClock

tiago_chassis = Primitive(id="tiago_chassis", namespace="robonix/primitive/chassis")

cmd_vel_pub = None  # rclpy publisher to /cmd_vel; created in init()
reference_clock = None
init_config = None
move_lock = threading.Lock()
command_lock = threading.Lock()
stopping = threading.Event()


# ── gRPC RPC: `move` (no MCP — keep velocity primitive off the LLM tool list) ─
import chassis_pb2  # noqa: E402  (proto codegen, on PYTHONPATH via rbnx-build/codegen/proto_gen)
import std_msgs_pb2  # noqa: E402


@tiago_chassis.grpc("robonix/primitive/chassis/move")
def move(req: "chassis_pb2.ExecuteMoveCommand_Request", context=None) -> "chassis_pb2.ExecuteMoveCommand_Response":
    def response(data):
        return chassis_pb2.ExecuteMoveCommand_Response(
            status=std_msgs_pb2.String(data=json.dumps(data)),
        )

    if not move_lock.acquire(blocking=False):
        return response({"error": "another chassis move is running"})
    try:
        return response(execute_move(req.command, context))
    except (ValueError, RuntimeError) as exc:
        return response({"error": str(exc)})
    finally:
        try:
            from geometry_msgs.msg import Twist  # type: ignore
            with command_lock:
                if cmd_vel_pub is not None:
                    cmd_vel_pub.publish(Twist())
        finally:
            move_lock.release()


def execute_move(msg, context):
    from geometry_msgs.msg import Twist  # type: ignore

    clock = reference_clock
    if cmd_vel_pub is None or clock is None or stopping.is_set():
        raise RuntimeError("ROS2 not initialized or chassis stopped")
    speed = float(os.environ.get("TIAGO_CHASSIS_SPEED_MPS", "0.3"))
    angular_speed = float(os.environ.get("TIAGO_CHASSIS_ANG_SPEED_RPS", "0.6"))
    default_duration = float(os.environ.get("TIAGO_CHASSIS_CMD_DURATION_SEC", "1.0"))
    if any(not math.isfinite(x) or x <= 0 for x in (speed, angular_speed, default_duration)):
        raise ValueError("chassis speed and default duration must be finite and positive")
    forward_m = float(getattr(msg, "forward_m", 0.0))
    rotate_deg = float(getattr(msg, "rotate_deg", 0.0))
    duration_sec = float(getattr(msg, "duration_sec", 0.0))
    if not all(math.isfinite(x) for x in (forward_m, rotate_deg, duration_sec)) or duration_sec < 0:
        raise ValueError("chassis distances and duration must be finite; duration must be nonnegative")
    tw = Twist()
    if forward_m != 0.0:
        tw.linear.x = math.copysign(speed, forward_m)
        duration = abs(forward_m) / speed
        mode = "forward_m"
    elif rotate_deg != 0.0:
        tw.angular.z = math.copysign(angular_speed, rotate_deg)
        duration = abs(math.radians(rotate_deg)) / angular_speed
        mode = "rotate_deg"
    else:
        for axis in ("linear", "angular"):
            for component in ("x", "y", "z"):
                value = float(getattr(msg, f"{axis}_{component}"))
                if not math.isfinite(value):
                    raise ValueError("chassis velocities must be finite")
                setattr(getattr(tw, axis), component, value)
        duration = duration_sec or default_duration
        mode = "velocity"
    if not math.isfinite(duration) or duration <= 0:
        raise ValueError("chassis duration must be finite and positive")
    start = clock.read()
    if start is None:
        raise RuntimeError("Chronos reference unavailable")
    while True:
        if stopping.is_set() or (context is not None and not context.is_active()):
            raise RuntimeError("chassis move canceled")
        current = clock.read()
        if current is None or current[1:] != start[1:] or current[0] < start[0]:
            raise RuntimeError("Chronos reference unavailable or reset during motion")
        if (current[0] - start[0]) / 1e9 >= duration:
            break
        with command_lock:
            if stopping.is_set() or cmd_vel_pub is None:
                raise RuntimeError("chassis stopped")
            cmd_vel_pub.publish(tw)
        stopping.wait(0.1)
    return {
        "status": "done", "mode": mode,
        "forward_m": forward_m, "rotate_deg": rotate_deg,
        "duration_sec": duration,
        "linear_x": tw.linear.x, "angular_z": tw.angular.z,
    }


# ── lifecycle ────────────────────────────────────────────────────────────────
@tiago_chassis.on_init
def init(cfg):
    global cmd_vel_pub, reference_clock, init_config
    cfg = cfg or {}
    if stopping.is_set():
        return Err("chassis stopped; restart the primitive")
    if init_config is not None:
        if cfg != init_config:
            return Err("chassis config changed while initialized; restart the primitive")
        return finish_init()
    reference_clock = _ReferenceClock(cfg.get("reference"), f"{tiago_chassis.id}/reference")
    odom_topic = cfg.get("odom_topic") or os.environ.get("TIAGO_ODOM_TOPIC", "/odom")
    twist_in_topic = cfg.get("twist_in_topic") or os.environ.get("TIAGO_CMD_VEL_TOPIC", "/cmd_vel")

    from geometry_msgs.msg import Twist  # type: ignore
    cmd_vel_pub = tiago_chassis.create_publisher(
        "robonix/primitive/chassis/twist_in",
        topic=twist_in_topic, msg_type=Twist, qos="reliable",
        declare=False,
    )
    tiago_chassis.declare_ros2_topic("robonix/primitive/chassis/twist_in", twist_in_topic, qos="reliable")
    tiago_chassis.declare_ros2_topic("robonix/primitive/chassis/odom",     odom_topic,     qos="reliable")
    init_config = dict(cfg)
    return finish_init()


def finish_init():
    if reference_clock.read() is None:
        return Deferred("waiting for Chronos reference time")
    return Ok()


@tiago_chassis.on_shutdown
def shutdown():
    global cmd_vel_pub
    stopping.set()
    try:
        from geometry_msgs.msg import Twist  # type: ignore
        with command_lock:
            try:
                if cmd_vel_pub is not None:
                    cmd_vel_pub.publish(Twist())
            finally:
                cmd_vel_pub = None
    finally:
        if reference_clock is not None:
            reference_clock.close()
    return Ok()


if __name__ == "__main__":
    tiago_chassis.run()
