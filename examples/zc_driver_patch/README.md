# ZC driver patch examples

These examples show the intended source-side ZC patch shape for C++ ROS 2
drivers. They are reference snippets for `patch-zc-driver`; they are not wired
into the workspace build.

The important distinction:

```text
Before: driver fills ROS Image/PointCloud2 -> publishes payload topic
After:  driver fills shared memory at the same production point -> publishes object-name topic
```

The generated sidecar owns Robonix registration and shared-memory lifecycle.
The patched driver must use the same values that appear in `sidecars/README.md`:

```cpp
static constexpr const char * kShmName = "robonix_zc_rgb";
static constexpr size_t kShmSize = 67'108'864;
static constexpr const char * kZcTopic = "/camera/rgb_zc";
```

If the current C++ API only exposes `shm_init(...)`, the driver may call
`shm_init(kShmName, kShmSize)` to open or create the same segment. The sidecar
and driver must both use the exact same segment name.

## Files

- `image_driver_before.cpp`: standard ROS 2 `Image` payload publisher.
- `image_driver_after.cpp`: source-side ZC version with legacy ROS fallback.
- `pointcloud_driver_before.cpp`: standard ROS 2 `PointCloud2` payload publisher.
- `pointcloud_driver_after.cpp`: source-side ZC version with legacy ROS fallback.

## CMake patch shape

Patch the real driver target, not a bridge node. Keep the style of the package's
existing `CMakeLists.txt`; this is the minimal shape:

```cmake
# @ai-filled source: src/camera_driver.cpp:Lx-Ly
find_package(cpp_pubsub REQUIRED)

target_link_libraries(camera_driver
  robonix_zc
)
```

If the package uses `ament_target_dependencies`, add the exported package there
as well when required by the local build:

```cmake
# @ai-filled source: src/camera_driver.cpp:Lx-Ly
ament_target_dependencies(camera_driver
  rclcpp
  sensor_msgs
  std_msgs
  cpp_pubsub
)
```

Add a `package.xml` dependency only when the package needs it:

```xml
<!-- @ai-filled source: src/camera_driver.cpp:Lx-Ly -->
<depend>cpp_pubsub</depend>
```

## Review checklist

- The patch is at the driver data production point.
- No new subscription to the original ROS payload topic was added.
- The sidecar and driver use the same `SHM_NAME`, `SHM_SIZE`, and `TOPIC`.
- Legacy ROS payload publishing is retained or guarded by an explicit fallback.
- The patched target links the ZC library.
