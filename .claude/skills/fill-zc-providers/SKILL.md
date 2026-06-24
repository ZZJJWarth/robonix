---
name: fill-zc-providers
description: Reads fill_manifest.json and fills TODO markers in generated ZC provider skeletons with business logic from matching source providers.
---

# fill-zc-providers

Reads `fill_manifest.json` (produced by `robonix-codegen --lang ros2_zc --fill`) and, for each entry that has a `source` match, reads both the source provider and the skeleton, then fills the TODO markers with extracted business logic.

## When to use

After running `robonix-codegen --lang ros2_zc --fill <providers_dir> -o gen/zc/`, when `fill_manifest.json` exists and has entries with `"source": "<path>"` (not null).

## What it does

For each manifest entry where `source` is not null:

1. **Read** the source provider file, the skeleton file, and the manifest entry's
   `msg_type` and `contract_id` fields.
2. **Verify msg_type consistency** — extract the message types used in the source
   provider (from `create_subscription` / `create_publisher` calls and their
   `msg_type=` arguments).  Compare against the manifest's `msg_type`:
   - If they match → continue to step 3.
   - If they **do not match** → skip filling, insert warning comments (see Key
     rules → Msg type consistency), and mark all TODOs as remaining in the
     summary.  This is the lidar3d/LaserScan-vs-PointCloud2 case.
3. **Identify** in the source:
   - Hardware SDK imports and initialisation (e.g. `camera.open(...)`, sensor library calls)
   - Frame / scan acquisition loops or callbacks
   - Error handling and retry logic
   - Configuration from env vars or cfg dict
   - Cleanup / close calls
   - Whether the source is itself a ROS subscriber (bridge pattern) or a direct
     hardware driver (source pattern)
4. **Map** each piece of business logic to the corresponding `# TODO:` marker in the skeleton:
   - "init hardware SDK / driver" → SDK initialisation, device handle creation, ROS subscriptions
   - "start frame capture thread / callback" → the subscriber callback or acquisition loop body
   - "stop frame capture thread / callback" → teardown of the acquisition thread
   - "close hardware" → device close / resource release
5. **Rewrite** the skeleton file with TODOs replaced by working code.
   - **Per-function marking**: Before each AI-filled function or block, insert a comment line:
     ```
     # @ai-filled — source: <source_file>:<lines>
     ```
     This lets reviewers see exactly which functions were touched and where the
     logic came from, at a glance.
   - The file header keeps the `# @generated` line; add a second line:
     ```
     # @ai-filled — review before commit
     ```
     to signal that the file as a whole contains AI-authored sections.
   - Any TODO that cannot be filled (no corresponding logic in source) stays as-is.
   - **SHM_NAME uniqueness**: rewrite `SHM_NAME` to include the contract leaf suffix
     (see Key rules).
   - **Bridge pattern**: when the source is itself a ROS subscriber, replace misleading
     hardware-oriented TODOs with accurate bridge comments (see Key rules).
   - **Imports**: remove `ZcSubscriber` if unused; add any imports the filled code needs.
   - **Lifecycle completeness**: ensure `shutdown()` destroys subscriptions created in
     `activate()`, even if the source provider doesn't explicitly do so.
6. **Report** which TODOs were filled and which remain for manual work.

## Key rules

- **Do not invent logic.** Only extract and adapt code that already exists in the source provider.
- **Preserve the ZC transport.** Keep `ZcPublisher.publish()` calls; do not revert to standard `rclpy.Publisher.publish()`.
- **Keep imports clean.** Add imports that the filled code needs; remove unused ones. In particular, remove `ZcSubscriber` from the import line if the filled provider only publishes (no subscribe path). Also remove `Deferred` if unused.
- **Never modify the source provider.** Only write to the skeleton file.
- **Mark EVERY filled function or block** with `# @ai-filled — source: <file>:<lines>`. A single file-level `@generated` / `@ai-filled` comment is not enough — reviewers must be able to see per-function provenance.

### Msg type consistency (CRITICAL)

Before filling any source-topic logic, **verify** that the source provider's message type matches
the ZC provider's `msg_type` from `fill_manifest.json`.

- If the source subscribes to `LaserScan` but the ZC contract expects `PointCloud2`, the
  `wait_for_topic` call will **never succeed** — the topic carries the wrong type.
- When there is a mismatch, **do not fill the source-topic TODOs**. Instead:
  - Leave `init()` / `activate()` TODOs in place.
  - Insert a warning comment above the affected TODOs:
    ```
    # @ai-filled — WARNING: source msg_type (LaserScan) != ZC msg_type (PointCloud2).
    # The source topic and msg type must be filled manually.
    ```

### SHM_NAME must be unique per contract

The codegen skeleton hardcodes `SHM_NAME = "robonix_zc"`.  When multiple ZC providers run
together, the **first one to call `shutdown_shm(...)` destroys the shared memory segment for
all others**.  Rewrite the `SHM_NAME` to include the contract leaf:

```
# Replace skeleton default:
SHM_NAME = "robonix_zc"                           # ← skeleton
SHM_NAME = "robonix_zc_rgb"                       # ← for rgb_zc
SHM_NAME = "robonix_zc_depth"                     # ← for depth_zc
SHM_NAME = "robonix_zc_lidar3d"                   # ← for lidar3d_zc
```

Derive the suffix from the last segment of the ZC contract id (the part after the final `/`,
minus the `_zc` suffix).  Example: `"robonix/primitive/camera/rgb_zc"` → suffix `"rgb"`.

### Architecture: bridge vs source detection

When the matched source provider **itself subscribes to a ROS 2 topic** (e.g. `tiago_camera`
listening on `/head_front_camera/rgb/image_raw`), the filled ZC provider becomes a
**bridge** — it receives data through DDS deserialization and republishes via shared memory.

This is a valid migration pattern, but the ZC speed-up is **one-sided** (producer still pays
DDS cost).  The `fill_manifest.json` TODOs (`"init hardware SDK / driver"`, `"close hardware"`)
are written for a direct-source provider, not a bridge.

When filling a **bridge** provider, replace those TODOs with accurate comments:

- `"init hardware SDK / driver"` → `# bridge: no SDK to init — data comes from source ROS topic`
- `"close hardware"`            → `# bridge: no hardware to close`
- `"start frame capture thread / callback"` → `# bridge: subscribe to source topic`
- `"stop frame capture thread / callback"`  → `# bridge: destroy source subscription`

### Lifecycle completeness

- **`shutdown()` must clean up what `activate()` created.**  If `activate()` creates a
  subscription, `shutdown()` must destroy it (guarded by `if _sub is not None`), in
  addition to calling `shutdown_shm(SHM_NAME)`.  Do not rely on `deactivate()` always
  being called before `shutdown()`.

- **`_sub` must have a type annotation** (e.g. `_sub: object | None = None` or the
  rclpy subscription type).  The skeleton's `_pub: ZcPublisher | None` is already
  annotated; keep `_sub` consistent.

- **Guard `_pub` usage.**  In the activate callback, check `if _pub is None: return`
  before calling `_pub.publish(msg)`, so a mis-ordered lifecycle doesn't crash the
  process.

## Output

After processing all entries, print a summary table:

```
## fill-zc-providers summary

| Skeleton | Source | TODOs filled | TODOs remaining |
|----------|--------|-------------|-----------------|
| camera_rgb_zc_provider.py | tiago_camera/driver.py | 4/4 | 0 |
| lidar_lidar3d_zc_provider.py | tiago_lidar/driver.py | 0/4 | 4 (msg_type mismatch — manual fill required) |
...

Review each @ai-filled function before committing.
```

If any entry was skipped due to msg_type mismatch, print a **WARNING** line after the table:

```
WARNING: lidar_lidar3d_zc msg_type=PointCloud2 but source (tiago_lidar) uses LaserScan — manual fill required.
```

## Prereqs

`fill_manifest.json` must exist. Run:
```bash
robonix-codegen --lang ros2_zc --contracts capabilities/ --fill <providers_dir> -o gen/zc/
```
to generate it.
