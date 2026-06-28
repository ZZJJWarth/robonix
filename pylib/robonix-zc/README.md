# robonix-zc

Python helpers for Robonix zero-copy ROS 2 message transport.

This package wraps the native shared-memory bridge used by generated ZC
providers and sidecars. It expects ROS 2 Python packages such as `rclpy`,
`sensor_msgs`, and `std_msgs` to come from the active ROS environment, for
example after sourcing `/opt/ros/<distro>/setup.bash`.

Do not add ROS 2 packages to `pyproject.toml` dependencies. They are not PyPI
packages, and adding them makes `uv sync` fail during workspace resolution.
