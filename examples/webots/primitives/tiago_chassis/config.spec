# Runtime configuration accepted by the Webots Tiago chassis primitive.
#
# This documents the mapping passed as the package instance's `config:` value.
# It is not loaded as a schema. Values below are runtime defaults.

config:
  # string absolute ROS 2 topic, default: /odom.
  # nav_msgs/Odometry source exposed through the chassis odometry capability.
  odom_topic: /odom

  # string absolute ROS 2 topic, default: /cmd_vel.
  # geometry_msgs/Twist command target used by chassis movement capabilities.
  twist_in_topic: /cmd_vel

  # Inherited from the deployment's Chronos selection under managed startup.
  # ros_time follows the Chronos reference stream; linux_system uses real
  # steady elapsed time. provider_id defaults to chronos for ros_time.
  reference:
    kind: linux_system

  # Launcher budget while waiting for the initial reference reading.
  # Seconds in the selected reference; default 90 unless overridden by
  # ROBONIX_DRIVER_INIT_TIMEOUT_S. Must be finite and positive.
  sentinel_timeout_s: 90.0
