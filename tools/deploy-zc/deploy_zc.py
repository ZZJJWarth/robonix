#!/usr/bin/env python3
# SPDX-License-Identifier: MulanPSL-2.0
"""deploy-zc: interactive deployer for generated ZC providers.

Scans a geneated-code directory (output of ``robonix-codegen --lang ros2_zc``),
presents an interactive menu, and deploys selected ZC providers into the
Robonix demo tree:

* Contracts   → ``capabilities/primitive/<domain>/``
* Provider .py → ``examples/webots/primitives/<pkg>/<driver>_driver/``
* Generates ``package_manifest.yaml``, ``scripts/build.sh``, ``scripts/start.sh``
* Optionally appends the new package to ``robonix_manifest.yaml``

Usage::

    python tools/deploy-zc/deploy_zc.py gen/zc/

    # non-interactive (scripting)
    python tools/deploy-zc/deploy_zc.py gen/zc/ --all --pkg tiago_camera_zc --dry-run
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
from pathlib import Path
from typing import Any

# ── helpers ──────────────────────────────────────────────────────────────────

def _msg_type_label(contract_id: str) -> str:
    leaf = contract_id.rsplit("/", 1)[-1].removesuffix("_zc")
    return leaf


def _contract_leaf(contract_id: str) -> str:
    """robonix/primitive/camera/rgb_zc → rgb_zc"""
    return contract_id.rsplit("/", 1)[-1]


def _contract_domain(contract_id: str) -> str:
    """robonix/primitive/camera/rgb_zc → camera"""
    parts = contract_id.split("/")
    return parts[-2] if len(parts) >= 2 else "unknown"


def _provider_module_name(contract_id: str) -> str:
    """robonix/primitive/camera/rgb_zc → camera_rgb_zc_provider"""
    parts = contract_id.split("/")
    domain = parts[-2] if len(parts) >= 2 else "unknown"
    leaf = parts[-1]
    return f"{domain}_{leaf}_provider"


def _scan_gen_dir(gen_dir: Path) -> list[dict[str, Any]]:
    """Walk *gen_dir*, read every fill_manifest.json, return merged entry list."""
    entries: list[dict[str, Any]] = []
    for mf in sorted(gen_dir.rglob("fill_manifest.json")):
        try:
            with open(mf) as f:
                data = json.load(f)
        except (json.JSONDecodeError, OSError) as exc:
            print(f"WARNING: skip {mf}: {exc}", file=sys.stderr)
            continue
        for entry in data:
            entry["_manifest_dir"] = str(mf.parent)
            entries.append(entry)
    return entries


def _resolve_gen_paths(entry: dict[str, Any], gen_dir: Path) -> dict[str, Path | None]:
    """Resolve skeleton .py and contract .toml paths from a manifest entry.

    The skeleton path in fill_manifest.json is relative to the CWD where
    codegen was run (typically the repo root).  We try, in order:
    1. Absolute path
    2. Relative to CWD (codegen convention)
    3. Relative to *gen_dir* (the generated-code root)
    4. Relative to the manifest file's parent directory
    """
    manifest_dir = Path(entry["_manifest_dir"])
    skeleton_rel = Path(entry["skeleton"])

    candidates: list[Path] = []
    if skeleton_rel.is_absolute():
        candidates.append(skeleton_rel)
    candidates.append((Path.cwd() / skeleton_rel).resolve())
    candidates.append((gen_dir / skeleton_rel).resolve())
    candidates.append((manifest_dir / skeleton_rel).resolve())

    skeleton: Path | None = None
    for c in candidates:
        if c.exists():
            skeleton = c
            break

    if skeleton is None and candidates:
        # None matched — use the most likely candidate for error reporting
        skeleton = candidates[0]

    # Contract TOML: relative to gen_dir, following the codegen output layout:
    #   <gen_dir>/<group>/contracts/<domain>/<leaf>.v1.toml
    contract_id: str = entry["contract_id"]
    domain = _contract_domain(contract_id)
    leaf = _contract_leaf(contract_id)
    toml_candidates = [
        manifest_dir / "contracts" / domain / f"{leaf}.v1.toml",
        gen_dir / "contracts" / domain / f"{leaf}.v1.toml",
    ]
    # also search recursively under gen_dir
    toml: Path | None = None
    for tc in toml_candidates:
        if tc.exists():
            toml = tc
            break
    if toml is None:
        hits = list(gen_dir.rglob(f"{leaf}.v1.toml"))
        if hits:
            toml = hits[0]

    return {"skeleton": skeleton, "contract": toml}


# ── interactive menu ─────────────────────────────────────────────────────────

def _interactive_select(entries: list[dict[str, Any]]) -> list[int]:
    """Show a numbered menu, return list of selected indices."""
    print("\nAvailable ZC providers:\n")
    for i, e in enumerate(entries):
        cid = e["contract_id"]
        msg = e.get("msg_type", "?")
        source = e.get("source") or "(no source — manual fill)"
        warn = " ⚠️  HAS WARNINGS" if _has_warnings(e) else ""
        print(f"  [{i}] {cid}")
        print(f"      msg_type: {msg}  source: {source}{warn}")
    print()

    while True:
        raw = input("Enter indices to deploy (comma/space-separated, 'all', 'q'): ").strip()
        if raw.lower() in ("q", "quit", ""):
            return []
        if raw.lower() == "all":
            return list(range(len(entries)))
        try:
            idxs = [int(x) for x in raw.replace(",", " ").split()]
            if all(0 <= i < len(entries) for i in idxs):
                return idxs
        except ValueError:
            pass
        print(f"  invalid — enter numbers 0–{len(entries)-1}, 'all', or 'q'")


def _has_warnings(entry: dict[str, Any]) -> bool:
    skeleton = Path(entry["skeleton"])
    if not skeleton.exists():
        return True
    text = skeleton.read_text()
    return "WARNING:" in text


def _ask_package_name(default: str) -> str:
    raw = input(f"Package name [{default}]: ").strip()
    return raw if raw else default


def _ask_confirm(msg: str) -> bool:
    raw = input(f"{msg} [Y/n]: ").strip().lower()
    return raw in ("", "y", "yes")


# ── deploy actions ───────────────────────────────────────────────────────────

def _deploy_contract(contract_path: Path, capabilities_dir: Path, dry_run: bool) -> bool:
    """Copy a ZC contract TOML into capabilities/primitive/<domain>/."""
    domain = contract_path.parent.name  # e.g. "camera"
    dest = capabilities_dir / "primitive" / domain / contract_path.name
    if dest.exists():
        print(f"  skip (exists): {dest}")
        return False
    if dry_run:
        print(f"  [dry-run] cp {contract_path} -> {dest}")
        return True
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(contract_path, dest)
    print(f"  cp {contract_path.name} -> {dest}")
    return True


def _deploy_provider(
    skeleton_path: Path,
    target_pkg_dir: Path,
    driver_name: str,
    dry_run: bool,
) -> Path | None:
    """Copy a single provider .py into the package driver directory.

    Returns the destination path, or None if the file already existed.
    """
    dest_dir = target_pkg_dir / f"{driver_name}_driver"
    dest = dest_dir / skeleton_path.name
    if dest.exists():
        print(f"  skip (exists): {dest}")
        return None
    if dry_run:
        print(f"  [dry-run] cp {skeleton_path.name} -> {dest}")
        return dest
    dest_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(skeleton_path, dest)
    print(f"  cp {skeleton_path.name} -> {dest}")
    return dest


# ── incremental merge helpers ────────────────────────────────────────────────

def _existing_manifest_caps(pkg_manifest: Path) -> set[str]:
    """Parse existing package_manifest.yaml, return set of contract ids."""
    if not pkg_manifest.exists():
        return set()
    text = pkg_manifest.read_text()
    caps: set[str] = set()
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("- name:"):
            cid = stripped.removeprefix("- name:").strip()
            if cid:
                caps.add(cid)
    return caps


def _existing_start_providers(start_sh: Path) -> list[str]:
    """Parse existing start.sh, return list of provider module names found."""
    if not start_sh.exists():
        return []
    text = start_sh.read_text()
    mods: list[str] = []
    for line in text.splitlines():
        # Look for: exec python3 -m <driver>_driver.<module>'
        if "exec python3 -m " in line:
            # Extract the module part after the last space and before the final '
            parts = line.split()
            for i, p in enumerate(parts):
                if p == "-m" and i + 1 < len(parts):
                    mod_full = parts[i + 1].rstrip("'")
                    # mod_full is like "camera_zc_driver.camera_rgb_zc_provider"
                    if "." in mod_full:
                        mods.append(mod_full.split(".")[-1])
    return mods


def _existing_start_env_vars(start_sh: Path) -> dict[str, str]:
    """Parse existing start.sh, extract env var defaults from docker exec lines.

    Skips hardcoded vars that are part of the template itself (ROBONIX_ATLAS, PYTHONPATH).
    """
    if not start_sh.exists():
        return {}
    import re
    text = start_sh.read_text()
    env: dict[str, str] = {}
    for m in re.finditer(r'-e (\w+)="\$\{\w+:-\{?([^}"\}]+)\}?"', text):
        key = m.group(1)
        if key in _TEMPLATE_VARS:
            continue
        env[key] = m.group(2)
    return env


# Env vars that are in the start.sh template — never treat as user-configurable.
_TEMPLATE_VARS = {"ROBONIX_ATLAS", "PYTHONPATH"}


def _render_package_manifest(pkg_name: str, contract_ids: list[str], description: str) -> str:
    caps = "\n".join(f"  - name: {cid}" for cid in contract_ids)
    return f"""# @generated by deploy-zc
manifestVersion: 1

package:
  name: com.robonix.example.{pkg_name}
  version: 0.1.0
  vendor: robonix
  description: {description}
  license: MulanPSL-2.0

build: bash scripts/build.sh
start: bash scripts/start.sh

capabilities:
{caps}
"""


def _render_build_sh(pkg_name: str) -> str:
    return f"""#!/usr/bin/env bash
# @generated by deploy-zc
set -euo pipefail
PKG="${{RBNX_PACKAGE_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}}"
echo "[{pkg_name}/build] ZC providers — no compiled deps; nothing to build."
echo "[{pkg_name}/build] done."
"""


def _render_start_sh(
    pkg_name: str,
    provider_files: list[str],
    driver_name: str,
    env_vars: dict[str, str],
) -> str:
    """Generate start.sh that launches each provider .py as a background process."""
    env_entries = [f'  -e {k}="${{{k}:-{v}}}" \\' for k, v in env_vars.items()]
    all_env = "\n".join(env_entries)
    launches = ""
    for pf in provider_files:
        mod = pf.removesuffix(".py")
        # Build the env block: if we have env vars, include them with continuation
        if all_env:
            env_block = all_env + "\n"
        else:
            env_block = ""
        launches += f"""docker exec -i -d \\
{env_block}  -e ROBONIX_ATLAS="${{ROBONIX_ATLAS:-127.0.0.1:50051}}" \\
  robonix_tiago_sim \\
  bash -lc 'set -eo pipefail
            source /opt/ros/humble/setup.bash
            zc_site=""
            for path in /robonix_pkgs/pylib/robonix-zc/install/robonix_zc/lib/python3*/site-packages; do
              if [ -d "$path" ]; then zc_site="$path"; break; fi
            done
            if [ -z "$zc_site" ]; then
              echo "[{pkg_name}] error: robonix_zc site-packages not found" >&2
              exit 1
            fi
            export PYTHONPATH="/robonix_pkgs/pylib/robonix-api:${{zc_site}}:${{PYTHONPATH:-}}"
            cd /robonix_pkgs/primitives/{pkg_name}
            exec python3 -m {driver_name}_driver.{mod}' &
"""
    return f"""#!/usr/bin/env bash
# @generated by deploy-zc
set -euo pipefail

if ! docker ps --format '{{{{.Names}}}}' | grep -qx robonix_tiago_sim; then
  echo "[{pkg_name}] error: sim container 'robonix_tiago_sim' is not running."
  echo "                Bring it up first:  bash examples/webots/sim/start.sh"
  exit 1
fi

cleanup() {{
  for name in {" ".join(pf.removesuffix('.py') for pf in provider_files)}; do
    docker exec robonix_tiago_sim pkill -9 -f "$name" 2>/dev/null || true
  done
  kill -- "-$$" 2>/dev/null || true
}}
trap cleanup EXIT INT TERM

echo "[{pkg_name}] launching ZC providers..."
{launches}
echo "[{pkg_name}] all providers launched."
wait
"""


def _update_manifest(
    manifest_path: Path,
    pkg_name: str,
    pkg_rel_path: str,
    dry_run: bool,
) -> bool:
    """Append a primitive entry to robonix_manifest.yaml."""
    if not manifest_path.exists():
        print(f"  WARNING: manifest not found at {manifest_path}")
        return False

    text = manifest_path.read_text()
    entry = f'\n  - name: {pkg_name}\n    path: {pkg_rel_path}\n    config: {{}}'
    if entry in text:
        print(f"  skip (already in manifest): {pkg_name}")
        return False

    # Insert before the "service:" section or at end of "primitive:" block
    marker = "\nservice:"
    if marker in text:
        new_text = text.replace(marker, f"{entry}{marker}", 1)
    else:
        new_text = text.rstrip() + f"\n{entry}\n"

    if dry_run:
        print(f"  [dry-run] append {pkg_name} to {manifest_path}")
        return True
    manifest_path.write_text(new_text)
    print(f"  appended {pkg_name} to {manifest_path}")
    return True


# ── main ─────────────────────────────────────────────────────────────────────

def main() -> None:
    ap = argparse.ArgumentParser(
        description="deploy-zc — interactive ZC provider deployment into the Robonix demo tree")
    ap.add_argument(
        "gen_dir", type=Path,
        help="Path to generated-code directory (output of robonix-codegen --lang ros2_zc)")
    ap.add_argument(
        "--pkg", dest="pkg_name", type=str, default=None,
        help="Target package name (default: auto-derived)")
    ap.add_argument(
        "--all", action="store_true",
        help="Deploy all discovered providers (non-interactive)")
    ap.add_argument(
        "--indices", type=str, default=None,
        help="Comma-separated provider indices to deploy (non-interactive)")
    ap.add_argument(
        "--capabilities-dir", type=Path,
        default=Path("capabilities"),
        help="Capabilities root (default: capabilities/)")
    ap.add_argument(
        "--primitives-dir", type=Path,
        default=Path("examples/webots/primitives"),
        help="Primitives root (default: examples/webots/primitives/)")
    ap.add_argument(
        "--manifest", type=Path,
        default=Path("examples/webots/robonix_manifest.yaml"),
        help="Path to robonix_manifest.yaml")
    ap.add_argument(
        "--no-manifest", action="store_true",
        help="Don't update robonix_manifest.yaml")
    ap.add_argument(
        "--dry-run", action="store_true",
        help="Print actions without touching the filesystem")
    args = ap.parse_args()

    gen_dir: Path = args.gen_dir.resolve()
    if not gen_dir.is_dir():
        print(f"error: gen_dir not found: {gen_dir}", file=sys.stderr)
        sys.exit(1)

    repo_root = Path.cwd()
    capabilities_dir = (repo_root / args.capabilities_dir).resolve()
    primitives_dir = (repo_root / args.primitives_dir).resolve()
    manifest_path = (repo_root / args.manifest).resolve()

    # ── scan ─────────────────────────────────────────────────────────────
    entries = _scan_gen_dir(gen_dir)
    if not entries:
        print("No fill_manifest.json entries found.")
        sys.exit(0)

    # resolve paths so the rest of the code doesn't need to guess
    for e in entries:
        paths = _resolve_gen_paths(e, gen_dir)
        e["_skeleton"] = paths["skeleton"]
        e["_contract"] = paths["contract"]

    # ── select ───────────────────────────────────────────────────────────
    if args.indices is not None:
        selected = [int(x) for x in args.indices.split(",")]
    elif args.all:
        selected = list(range(len(entries)))
    else:
        selected = _interactive_select(entries)

    if not selected:
        print("Nothing selected.  Bye.")
        sys.exit(0)

    chosen = [entries[i] for i in selected]
    contract_ids = [e["contract_id"] for e in chosen]

    # ── package name ─────────────────────────────────────────────────────
    if args.pkg_name:
        pkg_name = args.pkg_name
    else:
        # derive: first namespace + "_zc"
        ns = chosen[0].get("namespace", "unknown")
        ns_short = ns.split("/")[-1] if "/" in ns else ns
        default_name = f"{ns_short}_zc"
        if args.all:
            pkg_name = default_name
        else:
            pkg_name = _ask_package_name(default_name)

    target_dir = primitives_dir / pkg_name
    driver_name = pkg_name  # e.g. "tiago_camera_zc" → module "tiago_camera_zc_driver"

    # ── confirm ──────────────────────────────────────────────────────────
    print(f"\nDeploy plan:")
    print(f"  Package:     {pkg_name}")
    print(f"  Target dir:  {target_dir}")
    print(f"  Contracts:   {', '.join(_contract_leaf(c) for c in contract_ids)}")
    print(f"  Capabilities dir: {capabilities_dir}")
    print(f"  Update manifest:  {'no' if args.no_manifest else manifest_path}")
    print(f"  Mode:        {'DRY-RUN' if args.dry_run else 'LIVE'}")
    print()

    if not args.all and not args.indices and not _ask_confirm("Proceed?"):
        print("Cancelled.")
        sys.exit(0)

    # ── deploy contracts ─────────────────────────────────────────────────
    print("── Deploying contracts ──")
    for e in chosen:
        ct = e.get("_contract")
        if ct is not None:
            _deploy_contract(ct, capabilities_dir, args.dry_run)
        else:
            cid = e["contract_id"]
            print(f"  WARNING: no contract TOML found for {cid}")

    # ── deploy providers ─────────────────────────────────────────────────
    print("\n── Deploying providers ──")
    deployed_files: list[str] = []
    for e in chosen:
        sk = e.get("_skeleton")
        if sk is not None:
            dest = _deploy_provider(sk, target_dir, driver_name, args.dry_run)
            if dest is not None:
                deployed_files.append(sk.name)
            elif dest is not None or True:
                # Already exists — still track it for manifest/start.sh merging
                deployed_files.append(sk.name)
        else:
            print(f"  WARNING: no skeleton .py found for {e['contract_id']}")

    # ── __init__.py ──────────────────────────────────────────────────────
    init_py = target_dir / f"{driver_name}_driver" / "__init__.py"
    if init_py.exists():
        print(f"\n  skip (exists): {init_py}")
    elif not args.dry_run:
        init_py.parent.mkdir(parents=True, exist_ok=True)
        init_py.write_text("# ZC providers package\n")
        print(f"\n  wrote {init_py}")
    else:
        print(f"\n  [dry-run] write {init_py}")

    # ── package_manifest.yaml (merge with existing) ─────────────────────
    pkg_manifest = target_dir / "package_manifest.yaml"
    existing_caps = _existing_manifest_caps(pkg_manifest)
    all_caps = sorted(existing_caps | set(contract_ids))
    new_caps = set(contract_ids) - existing_caps

    if new_caps:
        caps_labels = sorted(set(_msg_type_label(c) for c in all_caps))
        desc = f"ZC transport bridge for {', '.join(caps_labels)}"
        manifest_content = _render_package_manifest(pkg_name, all_caps, desc)
        if not args.dry_run:
            target_dir.mkdir(parents=True, exist_ok=True)
            pkg_manifest.write_text(manifest_content)
            print(f"  {'updated' if existing_caps else 'wrote'} {pkg_manifest} (+{len(new_caps)} caps)")
        else:
            print(f"  [dry-run] {'update' if existing_caps else 'write'} {pkg_manifest}")
    else:
        print(f"  skip (no new caps): {pkg_manifest}")

    # ── scripts ──────────────────────────────────────────────────────────
    scripts_dir = target_dir / "scripts"
    if not args.dry_run:
        scripts_dir.mkdir(parents=True, exist_ok=True)

    build_sh = scripts_dir / "build.sh"
    if build_sh.exists():
        print(f"  skip (exists): {build_sh}")
    else:
        build_content = _render_build_sh(pkg_name)
        if not args.dry_run:
            build_sh.write_text(build_content)
            build_sh.chmod(0o755)
            print(f"  wrote {build_sh}")
        else:
            print(f"  [dry-run] write {build_sh}")

    # ── start.sh (merge with existing) ───────────────────────────────────
    start_sh = scripts_dir / "start.sh"
    existing_providers = _existing_start_providers(start_sh)
    all_providers = list(dict.fromkeys(existing_providers + [
        pf.removesuffix(".py") for pf in deployed_files
    ]))
    new_pf_count = len(set(pf.removesuffix(".py") for pf in deployed_files) -
                        set(existing_providers))

    if new_pf_count > 0:
        # Merge env vars from both existing start.sh and new skeletons
        env_vars = _existing_start_env_vars(start_sh)
        for e in chosen:
            sk = e.get("_skeleton")
            if sk is None or not sk.exists():
                continue
            text = sk.read_text()
            import re
            for m in re.finditer(r'os\.environ\.get\("(\w+)",\s*"([^"]+)"\)', text):
                if m.group(1) not in env_vars:
                    env_vars[m.group(1)] = m.group(2)

        provider_files = [f"{m}.py" for m in all_providers]
        start_content = _render_start_sh(pkg_name, provider_files, driver_name, env_vars)
        if not args.dry_run:
            start_sh.write_text(start_content)
            start_sh.chmod(0o755)
            print(f"  {'updated' if existing_providers else 'wrote'} {start_sh} (+{new_pf_count} providers)")
        else:
            print(f"  [dry-run] {'update' if existing_providers else 'write'} {start_sh}")
    else:
        print(f"  skip (no new providers): {start_sh}")

    # ── update robonix_manifest.yaml ──────────────────────────────────────
    if not args.no_manifest:
        print("\n── Updating robonix_manifest.yaml ──")
        _update_manifest(manifest_path, pkg_name, f"./primitives/{pkg_name}", args.dry_run)

    # ── done ──────────────────────────────────────────────────────────────
    print(f"\nDone.  {'DRY-RUN — no files touched.' if args.dry_run else ''}")
    if not args.dry_run:
        print(f"  Review generated files under: {target_dir}")
        print(f"  Contracts under:             {capabilities_dir / 'primitive'}")
        if not args.no_manifest:
            print(f"  Manifest updated:            {manifest_path}")


if __name__ == "__main__":
    main()
