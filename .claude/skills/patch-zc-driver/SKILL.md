---
name: patch-zc-driver
description: Reads zc_driver_patch_manifest.json and patches C++ driver source so data is written to shared memory at the driver production point.
---

# patch-zc-driver

Reads `zc_driver_patch_manifest.json` produced by
`robonix-codegen --lang ros2_zc --fill <driver_source_dir>`, then patches C++
driver source so `Image` / `PointCloud2` data is written to Boost shared memory
before the standard ROS 2 payload publish path.

## Scope

This skill is for **driver-source ZC**. It modifies the data production path in
the driver itself. The sidecar only registers the Robonix capability and owns
the shared-memory lifecycle.

This skill must not create a bridge that subscribes to an existing ROS 2 payload
topic and republishes via shared memory. That bridge pattern belongs to
`fill-zc-providers` and does not remove the original driver publish cost.

## Inputs

The user should provide the path to `zc_driver_patch_manifest.json`. For each
manifest entry, read:

- `contract_id`
- `msg_type`
- `source_dir` / `source_dirs`
- `candidate_files`
- `shm_name`
- `shm_size`
- `topic`
- `sidecar`
- `rules`
- `warnings`

Also read the generated sidecar file and the available C++ ZC API references
when present:

- `zc_pubsub.hpp`
- `zc_shm.hpp`
- the existing C++ driver `CMakeLists.txt`
- the existing `package.xml`, if this is a ROS package

## Required Workflow

For each manifest entry:

1. **Read manifest and candidates**
   - Read the full manifest entry.
   - Read every file in `candidate_files`.
   - If `candidate_files` is empty, search `source_dirs` for `.cpp`, `.cc`,
     `.cxx`, and `.hpp` files before editing.
   - Respect manifest warnings; do not treat a warning as proof that the patch
     point is known.

2. **Find the real data production point**
   - Look for SDK callbacks, frame read loops, lidar packet conversion, timer
     callbacks, or the point where `sensor_msgs::msg::Image` /
     `sensor_msgs::msg::PointCloud2` has just been filled.
   - Prefer the code immediately before `publisher_->publish(msg)` or
     equivalent standard ROS payload publish calls.
   - Confirm the message type matches manifest `msg_type` before patching.
   - If multiple plausible publish points exist, patch none of them until the
     ambiguity is recorded in the final output.

3. **Patch C++ source at the publish point**
   - Add the minimal includes required by the local ZC API, normally
     `zc_pubsub.hpp` and/or `zc_shm.hpp`.
   - Initialize or connect to shared memory using manifest `shm_name` and
     `shm_size`, following the local C++ API conventions.
   - Create or hold a ZC publisher for manifest `topic`.
   - Convert the filled ROS message into `ShmImage` or `ShmPointCloud2`.
   - Publish the shared-memory object name on the ZC object-name topic.
   - Keep lifecycle changes local to existing init/start/stop/shutdown points.

4. **Preserve legacy ROS publishing**
   - Keep the existing ROS payload publisher unless the user explicitly asks to
     remove it.
   - If practical, guard legacy publishing with an existing parameter, env var,
     or a small clearly named config flag.
   - Do not change existing topic names, parameters, or launch behavior unless
     required for the ZC patch.

5. **Patch build files**
   - Update `CMakeLists.txt` to find and link the package that exports
     `robonix_zc`.
   - Add include/link configuration using existing CMake style.
   - Update `package.xml` only when the package needs an explicit dependency.
   - Keep changes minimal and local to the patched target.

6. **Mark every modified block**
   - Before every AI-filled C++ block, add:
     ```cpp
     // @ai-filled source: <source_file>:Lx-Ly
     ```
   - For CMake/package changes, use the nearest supported comment style:
     ```cmake
     # @ai-filled source: <source_file>:Lx-Ly
     ```
   - The marker should point to the original code block that motivated the
     patch, such as the publish call or frame fill logic.

7. **Report result**
   - List edited files.
   - List the publish point that was patched.
   - List any uncertainty or manual follow-up.
   - Provide validation commands, including build commands and a runtime check
     that the generated sidecar can register `ROS2_ZC`.

## Message Mapping Rules

### Image

When `msg_type` is `Image`, patch at the point where a
`sensor_msgs::msg::Image` is fully populated.

Map fields into `ShmImage`:

- `header.stamp.sec`
- `header.stamp.nanosec`
- `header.frame_id`
- `height`
- `width`
- `encoding`
- `is_bigendian`
- `step`
- `data`

Use the shared-memory container allocator conventions already used by
`ShmImage`. Do not store pointers to stack-owned ROS message buffers in shared
memory.

### PointCloud2

When `msg_type` is `PointCloud2`, patch at the point where a
`sensor_msgs::msg::PointCloud2` is fully populated.

Map fields into `ShmPointCloud2`:

- `header.stamp.sec`
- `header.stamp.nanosec`
- `header.frame_id`
- `height`
- `width`
- `fields`
- `is_bigendian`
- `point_step`
- `row_step`
- `is_dense`
- `data`

Copy every `PointField` into the shared-memory vector using the local
`ShmPointField` representation and allocator conventions.

## Forbidden Changes

- Do not add `create_subscription` to subscribe to the original ROS payload
  topic.
- Do not implement subscribe-and-republish bridge logic in the driver patch.
- Do not modify `system/atlas/proto/atlas.proto`, contract TOML schema, or
  `pylib/robonix-api/` public API.
- Do not rename contracts, capability namespaces, lifecycle states, or wire
  fields.
- Do not rewrite the driver lifecycle without evidence from the existing code.
- Do not delete existing driver behavior unless the user explicitly confirms it.
- Do not hide uncertainty. Leave a warning and stop before editing ambiguous
  publish paths.

## Validation Checklist

After patching, verify and report:

- The patched source contains shared-memory writes at the driver production
  point.
- The patch did not add subscription-to-original-topic bridge logic.
- `CMakeLists.txt` links the ZC library for the patched target.
- The sidecar and driver use the same `SHM_NAME`, `SHM_SIZE`, and `TOPIC`.
- The project builds with the package-local build command.
- The sidecar can register the `ROS2_ZC` capability.
- A consumer can receive object names from the ZC topic and read from shared
  memory.

## Output Format

End with:

```markdown
## patch-zc-driver summary

Edited files:
- path/to/driver.cpp
- path/to/CMakeLists.txt

Patched publish point:
- path/to/driver.cpp:Lx-Ly

Validation:
- <command>  # result or not run

Warnings:
- <remaining uncertainty, or "none">
```

If no unambiguous driver production point is found, make no edits and report the
reason.
