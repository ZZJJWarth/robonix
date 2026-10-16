# TIAGo chassis primitive

Publishes velocity commands and exposes odometry through the Webots deployment.
Build and start it with that deployment's manifest.

Movement uses distance divided by speed, rotation divided by angular speed,
or the requested `duration_sec`. These durations follow the selected reference:
`ros_time` consumes Chronos readings through Atlas and stays fixed during a
pause; `linux_system` uses real steady elapsed time. Command refresh and
cancellation checks run every 100 real milliseconds. Stream sampling and
transport add timing error; this primitive does not provide position feedback.

INIT returns `Deferred` until the reference is available. Concurrent moves are
rejected. Caller cancellation, shutdown, reference loss or a generation change
stops movement and sends zero velocity. Interrupted moves return an error in
the existing response status JSON.

`TIAGO_CHASSIS_SPEED_MPS` defaults to `0.3`, `TIAGO_CHASSIS_ANG_SPEED_RPS` to
`0.6`, and `TIAGO_CHASSIS_CMD_DURATION_SEC` to `1.0`. All must be finite and
positive. See `config.spec` for topics and reference selection. Rebuild package
bindings and use the updated `robonix-api` before starting this primitive.
