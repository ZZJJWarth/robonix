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

1. **Read** the source provider file and the skeleton file.
2. **Identify** in the source:
   - Hardware SDK imports and initialisation (e.g. `camera.open(...)`, sensor library calls)
   - Frame / scan acquisition loops or callbacks
   - Error handling and retry logic
   - Configuration from env vars or cfg dict
   - Cleanup / close calls
3. **Map** each piece of business logic to the corresponding `# TODO:` marker in the skeleton:
   - "init hardware SDK / driver" → SDK initialisation, device handle creation, ROS subscriptions
   - "start frame capture thread / callback" → the subscriber callback or acquisition loop body
   - "stop frame capture thread / callback" → teardown of the acquisition thread
   - "close hardware" → device close / resource release
4. **Rewrite** the skeleton file with TODOs replaced by working code.
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
5. **Report** which TODOs were filled and which remain for manual work.

## Key rules

- **Do not invent logic.** Only extract and adapt code that already exists in the source provider.
- **Preserve the ZC transport.** Keep `ZcPublisher.publish()` calls; do not revert to standard `rclpy.Publisher.publish()`.
- **Keep imports clean.** Add imports that the filled code needs; remove unused ones.
- **Never modify the source provider.** Only write to the skeleton file.
- **Mark EVERY filled function or block** with `# @ai-filled — source: <file>:<lines>`. A single file-level `@generated` / `@ai-filled` comment is not enough — reviewers must be able to see per-function provenance.

## Output

After processing all entries, print a summary table:

```
## fill-zc-providers summary

| Skeleton | Source | TODOs filled | TODOs remaining |
|----------|--------|-------------|-----------------|
| camera_rgb_zc_provider.py | tiago_camera/driver.py | 3/4 | 1 (close hardware) |
| lidar_lidar3d_zc_provider.py | tiago_lidar/driver.py | 2/4 | 2 (init, close hardware) |
...

Review each @ai-filled function before committing.
```

## Prereqs

`fill_manifest.json` must exist. Run:
```bash
robonix-codegen --lang ros2_zc --contracts capabilities/ --fill <providers_dir> -o gen/zc/
```
to generate it.
