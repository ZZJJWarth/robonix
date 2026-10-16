# TIAGo lidar primitive

Exposes the normalized Webots LaserScan topic and the latest scan snapshot.
Build and start it through the Webots deployment; `config.spec` documents the
source topic and startup budget.

INIT establishes the subscription once and returns `Deferred` until its scan
callback receives a message. The managed launcher owns the first-frame budget
in the selected reference domain. Pausing `ros_time` does not expire it.
Shutdown remains available during the wait. The default budget is the
shared launcher's 90 seconds; explicit `sentinel_timeout_s` values take priority.
