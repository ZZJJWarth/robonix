#!/usr/bin/env bash
# SPDX-License-Identifier: MulanPSL-2.0
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WEBOTS_DIR="$(cd "$SCRIPT_DIR/../../examples/webots" && pwd)"
MANIFEST="$WEBOTS_DIR/.robonix_manifest.chronos.yaml"

if [[ "${1:-}" == --help || "${1:-}" == -h ]]; then
  echo "Usage: $0 [rbnx boot options]"
  echo "Start examples/webots/sim/start.sh first. Set ROBONIX_TIAGO_VARIANT=lite|full."
  exit 0
fi

export RMW_IMPLEMENTATION=rmw_zenoh_cpp
export ROBONIX_TIAGO_VARIANT="${ROBONIX_TIAGO_VARIANT:-lite}"
case "$ROBONIX_TIAGO_VARIANT" in
  lite) export ROBONIX_SOMA_ROBOT_YAML="$WEBOTS_DIR/soma.yaml" ;;
  full) export ROBONIX_SOMA_ROBOT_YAML="$WEBOTS_DIR/soma.full.yaml" ;;
  *) echo "Unsupported TIAGo variant: $ROBONIX_TIAGO_VARIANT (choose lite or full)" >&2; exit 2 ;;
esac

# Keep the manifest beside the original so package paths and Soma's Docker mount agree.
awk '
  { print }
  /^system:$/ {
    print "  chronos:"
    print "    listen: 127.0.0.1:50094"
    print "    provider_id: chronos"
    print "    core:"
    print "      reference:"
    print "        kind: linux_system"
  }
' "$WEBOTS_DIR/robonix_manifest.yaml" > "$MANIFEST"

cd "$WEBOTS_DIR"
exec rbnx boot -f "$MANIFEST" "$@"
