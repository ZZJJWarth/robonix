// SPDX-License-Identifier: MulanPSL-2.0
// Generate ZC contract TOMLs and Python provider skeletons from a list of
// `ZcCandidate`s produced by `zc_scanner`.

use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::zc_scanner::{ProviderInfo, ZcCandidate};

/// One entry in fill_manifest.json.
#[derive(serde::Serialize)]
struct FillManifestEntry {
    skeleton: String,
    source: Option<String>,
    contract_id: String,
    namespace: String,
    msg_type: String,
    generation_mode: &'static str,
    todos: Vec<&'static str>,
}

/// One entry in zc_driver_patch_manifest.json.
#[derive(serde::Serialize)]
struct DriverPatchManifestEntry {
    contract_id: String,
    msg_type: String,
    source_dir: Option<String>,
    source_dirs: Vec<String>,
    candidate_files: Vec<String>,
    shm_name: String,
    shm_size: u64,
    topic: String,
    sidecar: String,
    generation_mode: &'static str,
    rules: DriverPatchRules,
    warnings: Vec<String>,
}

/// Rules that downstream AI patching must obey when modifying driver source.
#[derive(serde::Serialize)]
struct DriverPatchRules {
    must_patch_driver_publish_point: bool,
    forbid_subscribe_and_republish: bool,
    keep_legacy_ros_publish_optional: bool,
}

/// Emit ZC TOML contracts + Python provider skeletons + optional
/// fill_manifest.json into `out_dir`.
pub fn generate(
    candidates: &[ZcCandidate],
    out_dir: &Path,
    fill_providers: &[ProviderInfo],
    fill_source_dirs: &[PathBuf],
    verbose: bool,
) -> Result<()> {
    let contracts_dir = out_dir.join("contracts");
    let providers_dir = out_dir.join("providers");
    let sidecars_dir = out_dir.join("sidecars");
    fs::create_dir_all(&contracts_dir)?;
    fs::create_dir_all(&providers_dir)?;
    fs::create_dir_all(&sidecars_dir)?;

    let mut n_contracts = 0usize;
    let mut n_providers = 0usize;
    let mut n_sidecars = 0usize;
    let mut manifest_entries: Vec<FillManifestEntry> = Vec::new();

    for cand in candidates {
        if verbose {
            eprintln!(
                "[robonix-codegen] ros2_zc: {} ({})",
                cand.contract_id, cand.msg_info.label
            );
        }

        // ── ZC contract TOML ────────────────────────────────────────────────
        let toml_path = contract_toml_path(&contracts_dir, cand);
        if toml_path.exists() {
            if verbose {
                eprintln!(
                    "[robonix-codegen] ros2_zc: skip existing {}",
                    toml_path.display()
                );
            }
        } else {
            let toml_content = render_contract_toml(cand);
            let parent = toml_path.parent().unwrap();
            fs::create_dir_all(parent)?;
            fs::write(&toml_path, &toml_content)
                .with_context(|| format!("write {}", toml_path.display()))?;
            n_contracts += 1;
        }

        // ── Python provider skeleton ────────────────────────────────────────
        let py_path = providers_dir.join(provider_filename(cand));
        if py_path.exists() {
            if verbose {
                eprintln!(
                    "[robonix-codegen] ros2_zc: skip existing {}",
                    py_path.display()
                );
            }
        } else {
            let py_content = render_provider(cand);
            fs::write(&py_path, &py_content)
                .with_context(|| format!("write {}", py_path.display()))?;
            n_providers += 1;
        }

        // ── Python sidecar skeleton ────────────────────────────────────────
        let sidecar_path = sidecars_dir.join(sidecar_filename(cand));
        if sidecar_path.exists() {
            if verbose {
                eprintln!(
                    "[robonix-codegen] ros2_zc: skip existing {}",
                    sidecar_path.display()
                );
            }
        } else {
            let sidecar_content = render_sidecar(cand);
            fs::write(&sidecar_path, &sidecar_content)
                .with_context(|| format!("write {}", sidecar_path.display()))?;
            n_sidecars += 1;
        }

        // ── fill manifest entry ────────────────────────────────────────────
        let source = fill_providers
            .iter()
            .find(|p| p.namespace == cand.namespace)
            .map(|p| p.path.to_string_lossy().to_string());

        manifest_entries.push(FillManifestEntry {
            skeleton: py_path.to_string_lossy().to_string(),
            source,
            contract_id: cand.zc_contract_id.clone(),
            namespace: cand.namespace.clone(),
            msg_type: cand.msg_info.label.to_string(),
            generation_mode: "python_bridge",
            todos: vec![
                "init hardware SDK / driver",
                "start frame capture thread / callback",
                "stop frame capture thread / callback",
                "close hardware",
            ],
        });
    }

    // ── write fill_manifest.json ───────────────────────────────────────────
    if !fill_providers.is_empty() {
        let manifest_path = out_dir.join("fill_manifest.json");
        let json = serde_json::to_string_pretty(&manifest_entries)?;
        fs::write(&manifest_path, &json)
            .with_context(|| format!("write {}", manifest_path.display()))?;
        eprintln!(
            "[robonix-codegen] ros2_zc: fill_manifest.json ({} entries, {} with source match)",
            manifest_entries.len(),
            manifest_entries
                .iter()
                .filter(|e| e.source.is_some())
                .count()
        );
    }

    let readme_path = sidecars_dir.join("README.md");
    let readme = render_sidecar_readme(candidates);
    fs::write(&readme_path, readme).with_context(|| format!("write {}", readme_path.display()))?;

    if !fill_source_dirs.is_empty() {
        write_driver_patch_manifest(candidates, out_dir, fill_source_dirs)?;
    }

    eprintln!(
        "[robonix-codegen] ros2_zc: {} contracts, {} providers, {} sidecars -> {}",
        n_contracts,
        n_providers,
        n_sidecars,
        out_dir.display()
    );
    Ok(())
}

// ── path helpers ──────────────────────────────────────────────────────────

/// Map contract id segments into the contracts output directory.
fn contract_toml_path(contracts_dir: &Path, cand: &ZcCandidate) -> std::path::PathBuf {
    // contract_id looks like "robonix/primitive/camera/rgb"
    // → contracts_dir/camera/rgb_zc.v1.toml
    let parts: Vec<&str> = cand.contract_id.split('/').collect();
    // parts: ["robonix", "primitive", "camera", "rgb"]
    // Domain dir: the segment before the last ("camera").
    // Leaf: the last segment ("rgb_zc").
    let leaf = format!("{}_zc", parts.last().copied().unwrap_or("unknown"));
    let domain = if parts.len() >= 2 {
        parts[parts.len() - 2]
    } else {
        "unknown"
    };

    contracts_dir.join(domain).join(format!("{leaf}.v1.toml"))
}

fn provider_filename(cand: &ZcCandidate) -> String {
    // "robonix/primitive/camera/rgb" -> "camera_rgb_zc_provider.py"
    let parts: Vec<&str> = cand.contract_id.split('/').collect();
    if parts.len() >= 3 {
        let domain = parts[parts.len() - 2];
        let leaf = parts[parts.len() - 1];
        format!("{domain}_{leaf}_zc_provider.py")
    } else {
        format!("{}_zc_provider.py", cand.contract_id.replace('/', "_"))
    }
}

fn sidecar_filename(cand: &ZcCandidate) -> String {
    format!("{}_sidecar.py", zc_module_stem(cand))
}

// ── driver patch manifest ──────────────────────────────────────────────────

/// Write a manifest for the future driver-source patch workflow. The manifest
/// does not modify driver files; it gives the AI patch skill the selected ZC
/// contract, shared-memory parameters, candidate C++ files, and hard rules.
fn write_driver_patch_manifest(
    candidates: &[ZcCandidate],
    out_dir: &Path,
    fill_source_dirs: &[PathBuf],
) -> Result<()> {
    let manifest_path = out_dir.join("zc_driver_patch_manifest.json");
    let mut entries: Vec<DriverPatchManifestEntry> = Vec::new();
    for cand in candidates {
        entries.push(driver_patch_manifest_entry(cand, fill_source_dirs)?);
    }
    let json = serde_json::to_string_pretty(&entries)?;
    fs::write(&manifest_path, json).with_context(|| format!("write {}", manifest_path.display()))
}

/// Build one driver patch manifest entry for a ZC contract. Candidate file
/// discovery is intentionally shallow and conservative; uncertain matches are
/// surfaced as warnings instead of pretending the correct patch point is known.
fn driver_patch_manifest_entry(
    cand: &ZcCandidate,
    fill_source_dirs: &[PathBuf],
) -> Result<DriverPatchManifestEntry> {
    let source_dirs = fill_source_dirs
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let mut warnings = Vec::new();
    let candidate_files = discover_cpp_candidate_files(fill_source_dirs, &cand.msg_info.msg_name)?;

    if candidate_files.is_empty() {
        warnings.push(format!(
            "no C++ driver candidate files found for msg_type {}; patch-zc-driver must locate the publish point manually",
            cand.msg_info.msg_name
        ));
    }

    Ok(DriverPatchManifestEntry {
        contract_id: cand.zc_contract_id.clone(),
        msg_type: cand.msg_info.msg_name.clone(),
        source_dir: source_dirs.first().cloned(),
        source_dirs,
        candidate_files,
        shm_name: shm_name(cand),
        shm_size: 67_108_864,
        topic: topic_name(cand),
        sidecar: format!("sidecars/{}", sidecar_filename(cand)),
        generation_mode: "driver_source_patch",
        rules: DriverPatchRules {
            must_patch_driver_publish_point: true,
            forbid_subscribe_and_republish: true,
            keep_legacy_ros_publish_optional: true,
        },
        warnings,
    })
}

/// Recursively scan source directories for likely C++ driver files. A file is
/// included when it has the selected message type, a ROS publisher call, or
/// obvious camera/lidar read-loop wording that may be the data production path.
fn discover_cpp_candidate_files(dirs: &[PathBuf], msg_name: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for dir in dirs {
        collect_cpp_candidate_files(dir, msg_name, &mut out)?;
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Walk a directory tree and append matching C++ source/header files. Missing
/// or non-directory inputs are ignored because `--fill` is also used for Python
/// provider trees that may not contain driver-source C++.
fn collect_cpp_candidate_files(dir: &Path, msg_name: &str, out: &mut Vec<String>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_cpp_candidate_files(&path, msg_name, out)?;
        } else if is_cpp_source(&path) && cpp_candidate_matches(&path, msg_name)? {
            out.push(path.to_string_lossy().to_string());
        }
    }
    Ok(())
}

fn is_cpp_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|x| x.to_str()),
        Some("cpp" | "cc" | "cxx" | "hpp")
    )
}

/// Check the small set of stable source markers used by the driver patch
/// workflow: message types, ROS publisher setup/calls, and obvious frame or
/// scan callback/read-loop names.
fn cpp_candidate_matches(path: &Path, msg_name: &str) -> Result<bool> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let msg_hit = match msg_name {
        "Image" => {
            text.contains("sensor_msgs::msg::Image")
                || text.contains("sensor_msgs/msg/image.hpp")
                || text.contains("sensor_msgs::msg::Image::")
        }
        "PointCloud2" => {
            text.contains("sensor_msgs::msg::PointCloud2")
                || text.contains("sensor_msgs/msg/point_cloud2.hpp")
                || text.contains("sensor_msgs::msg::PointCloud2::")
        }
        other => text.contains(&format!("sensor_msgs::msg::{other}")),
    };
    Ok(msg_hit
        || text.contains("create_publisher<")
        || text.contains("publisher_->publish")
        || text.contains(".publish(")
        || text.contains("frame_callback")
        || text.contains("image_callback")
        || text.contains("pointcloud_callback")
        || text.contains("scan_callback")
        || text.contains("read_frame")
        || text.contains("read_scan"))
}

// ── template: ZC contract TOML ──────────────────────────────────────────────

fn render_contract_toml(cand: &ZcCandidate) -> String {
    let desc = if cand.description.is_empty() {
        format!(
            "{} stream over zero-copy shared-memory transport.",
            msg_type_label(cand)
        )
    } else {
        format!("{} (ZC transport)", cand.description)
    };

    format!(
        r#"# Zero-copy {label} stream.
# Same IDL and mode as the standard {leaf} contract.
# Generated by robonix-codegen --lang ros2_zc.
[contract]
id      = "{zc_id}"
version = "1"
kind    = "{kind}"
idl     = "{idl}"
description = "{desc}"

[mode]
type = "{mode}"
"#,
        label = msg_type_label(cand),
        leaf = leaf_name(cand),
        zc_id = cand.zc_contract_id,
        kind = if cand.kind.is_empty() {
            "primitive"
        } else {
            &cand.kind
        },
        idl = cand.idl_path,
        desc = desc,
        mode = if cand.mode.is_empty() {
            "topic_out"
        } else {
            &cand.mode
        },
    )
}

// ── template: Python provider skeleton ──────────────────────────────────────

fn render_provider(cand: &ZcCandidate) -> String {
    let provider_id = provider_id(cand);
    let topic = topic_name(cand);
    let msg_label = msg_type_label(cand);
    let shm_name = "robonix_zc";
    let shm_size = 67_108_864u64; // 64 MiB

    let mut out = String::new();

    let _ = writeln!(out, "#!/usr/bin/env python3");
    let _ = writeln!(out, "# @generated by robonix-codegen --lang ros2_zc");
    let _ = writeln!(
        out,
        "# Zero-copy {msg_label} provider for contract `{zc_id}`.",
        zc_id = cand.zc_contract_id
    );
    let _ = writeln!(out, "# Generation mode: python_bridge.");
    let _ = writeln!(
        out,
        "# This compatibility bridge writes shared memory after receiving data in Python."
    );
    let _ = writeln!(
        out,
        "# Full source-side ZC requires patching the driver to write shared memory directly."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "from __future__ import annotations");
    let _ = writeln!(out);
    let _ = writeln!(out, "import logging");
    let _ = writeln!(out);
    let _ = writeln!(out, "from robonix_api import Primitive, Ok");
    let _ = writeln!(
        out,
        "from robonix_api.atlas_types import Transport, Ros2ZcParams"
    );
    let _ = writeln!(out, "from robonix_api.ros import RosBackend");
    let _ = writeln!(
        out,
        "from robonix_zc import ZcPublisher, ZcSubscriber, init_shm, shutdown_shm"
    );

    // Import the right ROS message type.
    let _ = writeln!(out, "{} as RosMsg", cand.msg_info.python_import);

    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "log = logging.getLogger(\"{provider_id}\")",
        provider_id = provider_id
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "SHM_NAME = \"{shm_name}\"");
    let _ = writeln!(out, "SHM_SIZE = {shm_size}  # 64 MiB");
    let _ = writeln!(out, "TOPIC    = \"{topic}\"");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "# ── provider ───────────────────────────────────────────────────────────────"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "provider = Primitive(");
    let _ = writeln!(out, "    id=\"{provider_id}\",");
    let _ = writeln!(
        out,
        "    namespace=\"{namespace}\",",
        namespace = cand.namespace
    );
    let _ = writeln!(out, ")");
    let _ = writeln!(out);
    let _ = writeln!(out, "_pub: ZcPublisher | None = None");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "# ── lifecycle ──────────────────────────────────────────────────────────────"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "@provider.on_init");
    let _ = writeln!(out, "def init(cfg: dict) -> Ok | str:");
    let _ = writeln!(out, "    global _pub");
    let _ = writeln!(
        out,
        "    log.info(\"initialising shared memory: name=%s size=%d\", SHM_NAME, SHM_SIZE)"
    );
    let _ = writeln!(out, "    init_shm(SHM_NAME, SHM_SIZE)");
    let _ = writeln!(out);
    let _ = writeln!(out, "    _pub = ZcPublisher(");
    let _ = writeln!(out, "        node=RosBackend.get().node,");
    let _ = writeln!(out, "        topic_name=TOPIC,");
    let _ = writeln!(out, "        msg_type=RosMsg,");
    let _ = writeln!(out, "        qos_profile=\"best_effort\",");
    let _ = writeln!(out, "    )");
    let _ = writeln!(out);
    let _ = writeln!(out, "    # Declare the ZC capability to Atlas");
    let _ = writeln!(out, "    provider.declare_capability(");
    let _ = writeln!(
        out,
        "        contract_id=\"{zc_id}\",",
        zc_id = cand.zc_contract_id
    );
    let _ = writeln!(out, "        endpoint=TOPIC,");
    let _ = writeln!(out, "        transport=Transport.ROS2_ZC,");
    let _ = writeln!(out, "        params=Ros2ZcParams(");
    let _ = writeln!(out, "            shm_name=SHM_NAME,");
    let _ = writeln!(out, "            shm_size=SHM_SIZE,");
    let _ = writeln!(out, "            qos_profile=\"best_effort\",");
    let _ = writeln!(out, "        ),");
    let _ = writeln!(
        out,
        "        description=\"{label} ZC stream\",",
        label = msg_type_label(cand)
    );
    let _ = writeln!(out, "    )");
    let _ = writeln!(
        out,
        "    log.info(\"declared {zc_id}\");",
        zc_id = cand.zc_contract_id
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "    # TODO: initialise your hardware SDK / driver here"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "    return Ok()");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "@provider.on_activate");
    let _ = writeln!(out, "def activate() -> Ok:");
    let _ = writeln!(out, "    # TODO: start frame capture thread / callback.");
    let _ = writeln!(out, "    # On each frame, call:");
    let _ = writeln!(out, "    #     msg = RosMsg()");
    let _ = writeln!(out, "    #     msg.data = ...   # raw payload");
    let _ = writeln!(out, "    #     _pub.publish(msg)");
    let _ = writeln!(out, "    return Ok()");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "@provider.on_deactivate");
    let _ = writeln!(out, "def deactivate() -> Ok:");
    let _ = writeln!(out, "    # TODO: stop frame capture thread / callback");
    let _ = writeln!(out, "    return Ok()");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "@provider.on_shutdown");
    let _ = writeln!(out, "def shutdown() -> None:");
    let _ = writeln!(out, "    # TODO: close hardware");
    let _ = writeln!(out, "    shutdown_shm(SHM_NAME)");
    let _ = writeln!(out, "    log.info(\"shared memory released\")");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "if __name__ == \"__main__\":");
    let _ = writeln!(
        out,
        "    logging.basicConfig(level=logging.INFO, format=\"%(asctime)s %(levelname)-5s %(name)s: %(message)s\")"
    );
    let _ = writeln!(out, "    provider.run()");

    out
}

/// Render a Python sidecar that owns Robonix registration and shared-memory
/// lifecycle only. The sidecar intentionally does not subscribe to or publish
/// ROS payload messages; the modified driver must write shared memory directly
/// and publish object names on `TOPIC`.
fn render_sidecar(cand: &ZcCandidate) -> String {
    let sidecar_id = sidecar_id(cand);
    let topic = topic_name(cand);
    let msg_label = msg_type_label(cand);
    let shm_name = shm_name(cand);
    let shm_size = 67_108_864u64; // 64 MiB

    let mut out = String::new();

    let _ = writeln!(out, "#!/usr/bin/env python3");
    let _ = writeln!(out, "# @generated by robonix-codegen --lang ros2_zc");
    let _ = writeln!(out, "# Sidecar for contract `{}`.", cand.zc_contract_id);
    let _ = writeln!(
        out,
        "# Data must be written by a patched driver, not by this sidecar."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "from __future__ import annotations");
    let _ = writeln!(out);
    let _ = writeln!(out, "import logging");
    let _ = writeln!(out);
    let _ = writeln!(out, "from robonix_api import Primitive, Ok");
    let _ = writeln!(
        out,
        "from robonix_api.atlas_types import Transport, Ros2ZcParams"
    );
    let _ = writeln!(out, "from robonix_zc import init_shm, shutdown_shm");
    let _ = writeln!(out);
    let _ = writeln!(out, "log = logging.getLogger(\"{sidecar_id}\")");
    let _ = writeln!(out);
    let _ = writeln!(out, "SHM_NAME = \"{shm_name}\"");
    let _ = writeln!(out, "SHM_SIZE = {shm_size}  # 64 MiB");
    let _ = writeln!(out, "TOPIC = \"{topic}\"");
    let _ = writeln!(out);
    let _ = writeln!(out, "sidecar = Primitive(");
    let _ = writeln!(out, "    id=\"{sidecar_id}\",");
    let _ = writeln!(out, "    namespace=\"{}\",", cand.namespace);
    let _ = writeln!(out, ")");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "@sidecar.on_init");
    let _ = writeln!(out, "def init(cfg: dict) -> Ok | str:");
    let _ = writeln!(
        out,
        "    log.info(\"initialising shared memory: name=%s size=%d\", SHM_NAME, SHM_SIZE)"
    );
    let _ = writeln!(out, "    init_shm(SHM_NAME, SHM_SIZE)");
    let _ = writeln!(out, "    sidecar.declare_capability(");
    let _ = writeln!(out, "        contract_id=\"{}\",", cand.zc_contract_id);
    let _ = writeln!(out, "        endpoint=TOPIC,");
    let _ = writeln!(out, "        transport=Transport.ROS2_ZC,");
    let _ = writeln!(out, "        params=Ros2ZcParams(");
    let _ = writeln!(out, "            shm_name=SHM_NAME,");
    let _ = writeln!(out, "            shm_size=SHM_SIZE,");
    let _ = writeln!(out, "            qos_profile=\"best_effort\",");
    let _ = writeln!(out, "        ),");
    let _ = writeln!(
        out,
        "        description=\"{msg_label} ZC stream from patched driver\","
    );
    let _ = writeln!(out, "    )");
    let _ = writeln!(
        out,
        "    log.info(\"declared {} on %s\", TOPIC)",
        cand.zc_contract_id
    );
    let _ = writeln!(out, "    return Ok()");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "@sidecar.on_activate");
    let _ = writeln!(out, "def activate() -> Ok:");
    let _ = writeln!(
        out,
        "    log.info(\"sidecar active; patched driver must publish object names on %s\", TOPIC)"
    );
    let _ = writeln!(out, "    return Ok()");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "@sidecar.on_deactivate");
    let _ = writeln!(out, "def deactivate() -> Ok:");
    let _ = writeln!(out, "    log.info(\"sidecar inactive\")");
    let _ = writeln!(out, "    return Ok()");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "@sidecar.on_shutdown");
    let _ = writeln!(out, "def shutdown() -> None:");
    let _ = writeln!(out, "    shutdown_shm(SHM_NAME)");
    let _ = writeln!(out, "    log.info(\"shared memory released\")");
    let _ = writeln!(out);
    let _ = writeln!(out);
    let _ = writeln!(out, "if __name__ == \"__main__\":");
    let _ = writeln!(
        out,
        "    logging.basicConfig(level=logging.INFO, format=\"%(asctime)s %(levelname)-5s %(name)s: %(message)s\")"
    );
    let _ = writeln!(out, "    sidecar.run()");

    out
}

/// Render the sidecar README with the shared-memory parameters each patched
/// driver must match. It is regenerated on every codegen run because it is a
/// derived summary of the selected contracts.
fn render_sidecar_readme(candidates: &[ZcCandidate]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# ZC sidecars");
    let _ = writeln!(out);
    let _ = writeln!(out, "Generated by `robonix-codegen --lang ros2_zc`.");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "These sidecars register `ROS2_ZC` capabilities and manage shared memory."
    );
    let _ = writeln!(
        out,
        "They do not subscribe to existing ROS payload topics or copy frames."
    );
    let _ = writeln!(
        out,
        "Patch the driver so it writes shared memory directly and publishes object names on the listed topic."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "| Contract | Sidecar | SHM_NAME | TOPIC |");
    let _ = writeln!(out, "| --- | --- | --- | --- |");
    for cand in candidates {
        let _ = writeln!(
            out,
            "| `{}` | `{}` | `{}` | `{}` |",
            cand.zc_contract_id,
            sidecar_filename(cand),
            shm_name(cand),
            topic_name(cand)
        );
    }
    out
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn msg_type_label(cand: &ZcCandidate) -> String {
    cand.msg_info.label.clone()
}

fn leaf_name(cand: &ZcCandidate) -> String {
    cand.contract_id
        .rsplit_once('/')
        .map(|(_, leaf)| leaf)
        .unwrap_or(&cand.contract_id)
        .to_string()
}

fn provider_id(cand: &ZcCandidate) -> String {
    // "robonix/primitive/camera/rgb" → "camera_rgb_zc_gen"
    let parts: Vec<&str> = cand.contract_id.split('/').collect();
    if parts.len() >= 3 {
        format!(
            "{}_{}_zc_gen",
            parts[parts.len() - 2],
            parts[parts.len() - 1]
        )
    } else {
        format!("{}_zc_gen", cand.contract_id.replace('/', "_"))
    }
}

fn sidecar_id(cand: &ZcCandidate) -> String {
    format!("{}_sidecar", zc_module_stem(cand))
}

/// Derive a per-contract shared-memory segment name for source-side ZC. The
/// suffix comes from the ZC contract leaf, so rgb_zc becomes robonix_zc_rgb.
fn shm_name(cand: &ZcCandidate) -> String {
    let leaf = cand
        .zc_contract_id
        .rsplit_once('/')
        .map(|(_, leaf)| leaf)
        .unwrap_or(&cand.zc_contract_id);
    let suffix = leaf.strip_suffix("_zc").unwrap_or(leaf);
    format!("robonix_zc_{suffix}")
}

/// Return a stable file/module stem like `camera_rgb_zc` for generated
/// sidecar-related filenames and identifiers.
fn zc_module_stem(cand: &ZcCandidate) -> String {
    let parts: Vec<&str> = cand.contract_id.split('/').collect();
    if parts.len() >= 3 {
        let domain = parts[parts.len() - 2];
        let leaf = parts[parts.len() - 1];
        format!("{domain}_{leaf}_zc")
    } else {
        format!("{}_zc", cand.contract_id.replace('/', "_"))
    }
}

fn topic_name(cand: &ZcCandidate) -> String {
    // "robonix/primitive/camera/rgb" → "/camera/rgb_zc"
    let parts: Vec<&str> = cand.contract_id.split('/').collect();
    if parts.len() >= 3 {
        let domain = parts[parts.len() - 2];
        let leaf = parts[parts.len() - 1];
        format!("/{domain}/{leaf}_zc")
    } else {
        format!("/{}_zc", cand.contract_id.replace('/', "_"))
    }
}
