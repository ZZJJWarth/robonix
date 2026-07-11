#!/usr/bin/env bash
set -euo pipefail

if ! docker ps --format '{{.Names}}' | grep -qx robonix_tiago_sim; then
  echo "[camera_zc] error: sim container 'robonix_tiago_sim' is not running."
  echo "                Bring it up first:  bash examples/webots/sim/start.sh"
  exit 1
fi

cleanup() {
  for name in camera_depth_zc_sidecar camera_rgb_zc_sidecar; do
    docker exec robonix_tiago_sim pkill -9 -f "$name" 2>/dev/null || true
  done
  kill -- "-$$" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "[camera_zc] launching ZC sidecars..."
docker exec -i -d \
  -e ROBONIX_ATLAS="${ROBONIX_ATLAS:-127.0.0.1:50051}" \
  robonix_tiago_sim \
  bash -lc 'set -eo pipefail
            source /opt/ros/humble/setup.bash
            zc_site=""
            for path in /robonix_pkgs/pylib/robonix-zc/install/robonix_zc/lib/python3*/site-packages; do
              if [ -d "$path" ]; then zc_site="$path"; break; fi
            done
            if [ -z "$zc_site" ]; then
              echo "[camera_zc] error: robonix_zc site-packages not found" >&2
              exit 1
            fi
            export PYTHONPATH="/robonix_pkgs/pylib/robonix-api:${zc_site}:${PYTHONPATH:-}"
            cd /robonix_pkgs/primitives/camera_zc
            exec python3 -m camera_zc_driver.camera_depth_zc_sidecar' &
docker exec -i -d \
  -e ROBONIX_ATLAS="${ROBONIX_ATLAS:-127.0.0.1:50051}" \
  robonix_tiago_sim \
  bash -lc 'set -eo pipefail
            source /opt/ros/humble/setup.bash
            zc_site=""
            for path in /robonix_pkgs/pylib/robonix-zc/install/robonix_zc/lib/python3*/site-packages; do
              if [ -d "$path" ]; then zc_site="$path"; break; fi
            done
            if [ -z "$zc_site" ]; then
              echo "[camera_zc] error: robonix_zc site-packages not found" >&2
              exit 1
            fi
            export PYTHONPATH="/robonix_pkgs/pylib/robonix-api:${zc_site}:${PYTHONPATH:-}"
            cd /robonix_pkgs/primitives/camera_zc
            exec python3 -m camera_zc_driver.camera_rgb_zc_sidecar' &

echo "[camera_zc] all sidecars launched."
wait
