# robonix-codegen

`robonix-codegen` turns Robonix contracts (TOML) + ROS-style IDL
(`.msg` / `.srv`) into language-specific stubs that components can
import directly. It runs as a library inside the Rust build (via the
`build.rs` of atlas / executor / liaison / pilot / rbnx) and as a CLI
that packages call from their own `scripts/build.sh`.

## What it generates

| Output | When | Where |
| --- | --- | --- |
| `*.proto` + tonic Rust stubs | Inside each Rust crate's `build.rs` | `OUT_DIR` |
| `proto_gen/*_pb2.py` + `*_pb2_grpc.py` | Python `rbnx codegen` | `<pkg>/rbnx-build/codegen/proto_gen/` |
| `robonix_mcp_types/<pkg>_mcp.py` | `rbnx codegen --mcp` | `<pkg>/rbnx-build/codegen/robonix_mcp_types/` |
| ZC contract TOMLs + Python bridge provider skeletons + sidecars | `robonix-codegen --lang ros2_zc` | Chosen `--out` directory |

The library API (`robonix_codegen::codegen::{msg_parser, contract_gen,
proto_gen}`) is what Rust callers use; the CLI is what Python packages
use via `rbnx codegen` (which wraps this binary).

## Build

From the repo root:

```sh
cargo build -p robonix-codegen
```

Installed to `~/.cargo/bin/robonix-codegen` by `make install`.

## Usage

Most users don't invoke `robonix-codegen` directly — they call
`rbnx codegen` from inside a package directory. The wrapper reads the
package's `package_manifest.yaml`, finds the contract files referenced
under `capabilities/`, and runs codegen with the right input/output
paths.

Direct invocation:

```sh
robonix-codegen \
    --idl-root <repo>/capabilities/lib \
    --contracts <repo>/capabilities --contracts <pkg>/capabilities \
    --out-dir <pkg>/rbnx-build/codegen/proto_gen
```

Add `--mcp` for the MCP types output (writes `robonix_mcp_types/` next
to `proto_gen/`).

## ROS2 ZC

`--lang ros2_zc` generates `_zc` contract TOMLs, Python bridge provider
skeletons, and Python sidecars. The generated Python bridge may subscribe
to an existing ROS 2 `Image` / `PointCloud2` topic and write the received
message into shared memory before publishing the object-name topic.

That bridge is a compatibility and demo path. It only optimizes the data
flow after the bridge receives the ROS message; the original driver still
pays the standard ROS 2 publish, serialization, DDS, and copy costs. Full
source-side ZC requires patching the driver data-production point so it
writes `ShmImage` / `ShmPointCloud2` directly.

The generated sidecar is the source-side integration piece. It registers
the `ROS2_ZC` capability with Atlas and manages the shared-memory segment,
but it does not subscribe to the original ROS payload topic or republish
frames. A patched driver must use the same `SHM_NAME` and `TOPIC` listed
in `sidecars/README.md`.

Direct invocation:

```sh
robonix-codegen \
    --lang ros2_zc \
    --contracts <repo>/capabilities \
    --filter robonix/primitive/camera/rgb \
    --fill <repo>/examples/webots/primitives \
    --out <repo>/gen/zc
```

When `--fill` is used, `fill_manifest.json` includes
`"generation_mode": "python_bridge"` so downstream tools and reviewers can
distinguish bridge generation from future driver-source patch workflows.
The same `--fill` directories are also scanned for C++ driver-source
candidates, and `zc_driver_patch_manifest.json` is emitted for the future
driver patch skill. That manifest is advisory: it does not modify source
files, and warnings mean the publish point must be located manually.

Output layout:

```text
<out>/
├── contracts/          # generated _zc contract TOMLs
├── providers/          # Python bridge provider skeletons
├── sidecars/           # Python sidecars for patched drivers
│   └── README.md       # SHM_NAME/TOPIC values each driver patch must match
├── fill_manifest.json  # only when --fill is used
└── zc_driver_patch_manifest.json  # driver-source patch inputs for AI/review
```
