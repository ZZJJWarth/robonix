# Runtime configuration accepted by the Webots Tiago planar lidar primitive.
#
# This documents the mapping passed as the package instance's `config:` value.
# It is not loaded as a schema. Values below are runtime defaults.

config:
  # string absolute ROS 2 topic, default: /scanner_normalized.
  # sensor_msgs/LaserScan source exposed through the planar lidar capability.
  scan_topic: /scanner_normalized

  # float seconds, default: 90.0; must be finite and greater than zero.
  # Launcher budget for Deferred INIT: reference seconds with ros_time,
  # real steady seconds otherwise. ROBONIX_DRIVER_INIT_TIMEOUT_S can override
  # the launcher default when this key is absent.
  sentinel_timeout_s: 90.0
