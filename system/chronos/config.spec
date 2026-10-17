specVersion: 1
description: >-
  Chronos publishes reference time, clock quality and device timestamp conversion
  models. Select ros_time for simulation; reference time then follows ROS 2
  /clock and freezes while the simulator is paused. Restart the deployment after
  changing the reference source. Deployments using this reference need Chronos
  enabled.

properties:
  listen:
    type: string
    default: 127.0.0.1:50094
    description: gRPC listen address, as IP:port.
  atlas_endpoint:
    type: string
    description: >-
      Atlas endpoint. rbnx uses system.atlas.listen when omitted. Standalone
      Chronos uses --atlas, then ROBONIX_ATLAS, then this field, then
      127.0.0.1:50051.
  provider_id:
    type: string
    default: chronos
    description: Atlas provider id. Must be non-empty without surrounding whitespace.
  log:
    type: string
    enum: [debug, info, warn, warning, error]
    description: File log level. When omitted, Scribe's environment and defaults apply.
  reference_reader_command:
    type: array
    items:
      type: string
    default: []
    description: >-
      Argument list for the ROS reference reader. Empty uses python3 -u with
      rclpy and rosgraph_msgs in a sourced ROS environment. Keep {script} as a
      literal argument; Chronos replaces it with the reader script path. A
      container wrapper must make that script available inside the container
      and keep stdin open, for example with docker exec -i. Preserve the Webots
      deployment's supplied command when using its simulator container.
  collect_observations:
    type: boolean
    default: false
    x-group: Observation collection
    description: >-
      Enable automatic collection for configured affine-model domains. This
      switch is independent of the ROS /clock reference reader.
  collect_ros_observations:
    type: boolean
    default: false
    x-group: Observation collection
    description: >-
      Also collect ROS timestamp streams when collect_observations is enabled.
      Requires ROS dependencies; does not control reading /clock.
  core:
    type: object
    description: >-
      Reference and device clock configuration. When omitted, Chronos selects
      linux_system with default quality settings and no device domains.
    required: [reference]
    properties:
      reference:
        type: object
        required: [kind]
        properties:
          kind:
            type: string
            enum: [linux_system, ros_time]
            default: linux_system
            description: >-
              linux_system reads Linux CLOCK_REALTIME. ros_time reads ROS 2
              /clock, remains unavailable until the first sample, and freezes
              during simulation pauses. Restart after changing this selection.
          max_uncertainty_ns:
            type: integer
            minimum: 1
            default: 1000000
            description: Maximum Linux reference uncertainty in nanoseconds.
          max_status_age_ns:
            type: integer
            minimum: 1
            default: 2000000000
            description: Maximum Linux reference sample age in nanoseconds.
          max_clock_step_ns:
            type: integer
            minimum: 1
            default: 100000000
            description: >-
              Linux clock step detection limit in nanoseconds. ROS reference
              time follows received steps without applying this limit.
      quality:
        type: object
        properties:
          refresh_interval_ns:
            type: integer
            minimum: 1
            default: 1000000000
            description: >-
              Quality refresh interval in real nanoseconds. ROS reference
              publication remains at most 100 real milliseconds apart.
          warning_interval_ns:
            type: integer
            minimum: 1
            default: 10000000000
            description: Minimum interval between quality warnings in real nanoseconds.
      domains:
        type: object
        default: {}
        description: >-
          Device domain declarations keyed by domain id. See the Chronos README
          for timestamp, capability and strategy settings. Keep empty for a
          reference-only deployment. ros_time supports direct domains;
          affine-model domains require linux_system.
