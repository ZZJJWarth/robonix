// SPDX-License-Identifier: MulanPSL-2.0
// Generate ZC contract TOMLs and Python provider skeletons from a list of
// `ZcCandidate`s produced by `zc_scanner`.

use anyhow::{Context, Result};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use super::zc_scanner::{ProviderInfo, ZcCandidate};

/// One entry in fill_manifest.json.
#[derive(serde::Serialize)]
struct FillManifestEntry {
    skeleton: String,
    source: Option<String>,
    contract_id: String,
    namespace: String,
    msg_type: String,
    todos: Vec<&'static str>,
}

/// Emit ZC TOML contracts + Python provider skeletons + optional
/// fill_manifest.json into `out_dir`.
pub fn generate(
    candidates: &[ZcCandidate],
    out_dir: &Path,
    fill_providers: &[ProviderInfo],
    verbose: bool,
) -> Result<()> {
    let contracts_dir = out_dir.join("contracts");
    let providers_dir = out_dir.join("providers");
    fs::create_dir_all(&contracts_dir)?;
    fs::create_dir_all(&providers_dir)?;

    let mut n_contracts = 0usize;
    let mut n_providers = 0usize;
    let mut manifest_entries: Vec<FillManifestEntry> = Vec::new();

    for cand in candidates {
        if verbose {
            eprintln!(
                "[robonix-codegen] ros2_zc: {} ({})",
                cand.contract_id,
                cand.msg_info.label
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
            manifest_entries.iter().filter(|e| e.source.is_some()).count()
        );
    }

    eprintln!(
        "[robonix-codegen] ros2_zc: {} contracts, {} providers -> {}",
        n_contracts,
        n_providers,
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

    contracts_dir
        .join(domain)
        .join(format!("{leaf}.v1.toml"))
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
        kind = if cand.kind.is_empty() { "primitive" } else { &cand.kind },
        idl = cand.idl_path,
        desc = desc,
        mode = if cand.mode.is_empty() { "topic_out" } else { &cand.mode },
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
    let _ = writeln!(out, "# Zero-copy {msg_label} provider for contract `{zc_id}`.", zc_id = cand.zc_contract_id);
    let _ = writeln!(out);
    let _ = writeln!(out, "from __future__ import annotations");
    let _ = writeln!(out);
    let _ = writeln!(out, "import logging");
    let _ = writeln!(out);
    let _ = writeln!(out, "from robonix_api import Primitive, Ok");
    let _ = writeln!(out, "from robonix_api.atlas_types import Transport, Ros2ZcParams");
    let _ = writeln!(out, "from robonix_api.ros import RosBackend");
    let _ = writeln!(out, "from robonix_zc import ZcPublisher, ZcSubscriber, init_shm, shutdown_shm");

    // Import the right ROS message type.
    let _ = writeln!(out, "{} as RosMsg", cand.msg_info.python_import);

    let _ = writeln!(out);
    let _ = writeln!(out, "log = logging.getLogger(\"{provider_id}\")", provider_id = provider_id);
    let _ = writeln!(out);
    let _ = writeln!(out, "SHM_NAME = \"{shm_name}\"");
    let _ = writeln!(out, "SHM_SIZE = {shm_size}  # 64 MiB");
    let _ = writeln!(out, "TOPIC    = \"{topic}\"");
    let _ = writeln!(out);
    let _ = writeln!(out, "# ── provider ───────────────────────────────────────────────────────────────");
    let _ = writeln!(out);
    let _ = writeln!(out, "provider = Primitive(");
    let _ = writeln!(out, "    id=\"{provider_id}\",");
    let _ = writeln!(out, "    namespace=\"{namespace}\",", namespace = cand.namespace);
    let _ = writeln!(out, ")");
    let _ = writeln!(out);
    let _ = writeln!(out, "_pub: ZcPublisher | None = None");
    let _ = writeln!(out);
    let _ = writeln!(out, "# ── lifecycle ──────────────────────────────────────────────────────────────");
    let _ = writeln!(out);
    let _ = writeln!(out, "@provider.on_init");
    let _ = writeln!(out, "def init(cfg: dict) -> Ok | str:");
    let _ = writeln!(out, "    global _pub");
    let _ = writeln!(out, "    log.info(\"initialising shared memory: name=%s size=%d\", SHM_NAME, SHM_SIZE)");
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
    let _ = writeln!(out, "        contract_id=\"{zc_id}\",", zc_id = cand.zc_contract_id);
    let _ = writeln!(out, "        endpoint=TOPIC,");
    let _ = writeln!(out, "        transport=Transport.ROS2_ZC,");
    let _ = writeln!(out, "        params=Ros2ZcParams(");
    let _ = writeln!(out, "            shm_name=SHM_NAME,");
    let _ = writeln!(out, "            shm_size=SHM_SIZE,");
    let _ = writeln!(out, "            qos_profile=\"best_effort\",");
    let _ = writeln!(out, "        ),");
    let _ = writeln!(out, "        description=\"{label} ZC stream\",", label = msg_type_label(cand));
    let _ = writeln!(out, "    )");
    let _ = writeln!(out, "    log.info(\"declared {zc_id}\");", zc_id = cand.zc_contract_id);
    let _ = writeln!(out);
    let _ = writeln!(out, "    # TODO: initialise your hardware SDK / driver here");
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
    let _ = writeln!(out, "    logging.basicConfig(level=logging.INFO, format=\"%(asctime)s %(levelname)-5s %(name)s: %(message)s\")");
    let _ = writeln!(out, "    provider.run()");

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
