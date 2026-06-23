# ZC Camera Demo

Zero-copy RGB camera capability — the reference implementation for
`robonix/primitive/camera/rgb_zc`.

## Files

| File | Purpose |
|------|---------|
| `zc_camera_provider.py` | Primitive that publishes 1080p mock frames via shared memory |
| `zc_camera_consumer.py` | Standalone subscriber — receives frames and prints FPS/size |

## Run (standalone, no Atlas required)

```bash
# Terminal 1 — consumer
python zc_camera_consumer.py

# Terminal 2 — provider
python zc_camera_provider.py
```

## Run (full Robonix stack with Atlas)

```bash
# Terminal 1 — Atlas
atlas

# Terminal 2 — boot lifecycle
rbnx boot --manifest examples/zc_demo/robonix_manifest.yaml

# Terminal 3 — consumer (standalone ZC)
python zc_camera_consumer.py
```

## What happens on the wire

```
Provider                          Consumer
────────                          ────────
Image (6 MB) → shm write          String "ShmObject_42" arrives
                │                  │
                ▼                  ▼
ROS String topic:            shm lookup → Image (6 MB) → callback
  "ShmObject_42" (20 bytes)
```

The ROS 2 topic carries only object names (~20 bytes).  The full 6 MB
frame is read directly from shared memory.
