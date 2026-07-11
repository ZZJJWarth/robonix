# robonix-zc

Python lifecycle helpers for Robonix zero-copy shared memory.

This package exposes `init_shm` and `shutdown_shm` for generated sidecars.
Frame data is written and read by the driver-side zero-copy implementation,
not by Python ROS 2 topic subscribers.

Do not add ROS 2 packages to `pyproject.toml` dependencies. They are not PyPI
packages, and adding them makes `uv sync` fail during workspace resolution.
