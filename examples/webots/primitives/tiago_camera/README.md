# TIAGo camera primitive

Exposes RGB/depth topics, JPEG snapshots and camera calibration from the
Webots ROS graph. Build and start it through the Webots deployment.

INIT establishes subscriptions once and returns `Deferred` until its RGB
callback has converted a frame. Managed launchers retry INIT and enforce
`sentinel_timeout_s` (default 90). With Chronos `ros_time`, this is a reference
time budget: paused simulation does not expire it, and a generation change
restarts the wait. Each lifecycle RPC retains a real deadline for transport or
handler failures. Shutdown remains available while waiting for a frame.

Camera extrinsics discovery waits up to 60 reference seconds. A reference generation
change restarts that wait; shutdown stops the transform and calibration workers.
Snapshot headers retain the source image's timestamp and frame, including a
valid zero timestamp. Calibration republishing and warnings use real time.
Use the updated `robonix-api` and rebuild package bindings before starting.

See `config.spec` for topics, calibration and timeout settings. Run
`python -m pytest tests` with the package's generated protobuf/MCP bindings on
`PYTHONPATH` to check deferred initialization.
