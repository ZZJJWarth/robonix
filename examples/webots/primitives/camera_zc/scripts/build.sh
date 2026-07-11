#!/usr/bin/env bash
set -euo pipefail
PKG="${RBNX_PACKAGE_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
echo "[camera_zc/build] ZC sidecars; no compiled deps; nothing to build."
echo "[camera_zc/build] done."
