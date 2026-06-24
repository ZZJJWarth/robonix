// SPDX-License-Identifier: MulanPSL-2.0
// Scan capability contract TOMLs and produce ZC candidates filtered by the
// user's `--filter` argument.  No hardcoded type list — the developer
// decides which contracts to upgrade.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

// ── TOML parse structures ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ContractToml {
    contract: ContractMeta,
    mode: ModeSpec,
}

#[derive(Debug, Deserialize)]
struct ContractMeta {
    id: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    idl: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModeSpec {
    #[serde(default, rename = "type")]
    mode_type: Option<String>,
}

// ── public types ────────────────────────────────────────────────────────────

/// Parsed ROS message type info — extracted from the IDL path.
#[derive(Debug, Clone, PartialEq)]
pub struct ZcMsgInfo {
    /// ROS package, e.g. "sensor_msgs".
    pub ros_pkg: String,
    /// Message name without extension, e.g. "Image".
    pub msg_name: String,
    /// Python import: `from sensor_msgs.msg import Image`.
    pub python_import: String,
    /// The class name for use in Python code.
    pub python_class: String,
    /// Human-readable label.
    pub label: String,
}

/// One contract that the user chose to generate a ZC variant for.
#[derive(Debug, Clone)]
pub struct ZcCandidate {
    /// Original contract id, e.g. "robonix/primitive/camera/rgb".
    pub contract_id: String,
    /// New ZC contract id, e.g. "robonix/primitive/camera/rgb_zc".
    pub zc_contract_id: String,
    /// The ROS IDL path from the TOML, e.g. "common_interfaces/sensor_msgs/msg/Image.msg".
    pub idl_path: String,
    /// Parsed message type info.
    pub msg_info: ZcMsgInfo,
    /// Contract namespace (everything before the last '/'), e.g. "robonix/primitive/camera".
    pub namespace: String,
    /// Contract kind: "primitive" | "service" | "skill".
    pub kind: String,
    /// Mode type: "topic_out" | "topic_in" | "rpc".
    pub mode: String,
    /// Source TOML path.
    pub source_toml: PathBuf,
    /// Contract description.
    pub description: String,
}

// ── scan ────────────────────────────────────────────────────────────────────

/// Walk every contract under `contracts_dirs`, return ALL contracts that
/// have an `idl` field (no hardcoded type filter — the caller applies
/// `--filter` to narrow down).
pub fn scan_contracts(contracts_dirs: &[PathBuf]) -> Result<Vec<ZcCandidate>> {
    let mut by_id: BTreeMap<String, (PathBuf, ContractToml)> = BTreeMap::new();
    for d in contracts_dirs {
        for p in collect_tomls(d)? {
            let raw =
                fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
            let c: ContractToml =
                toml::from_str(&raw).with_context(|| format!("parse {}", p.display()))?;
            by_id.insert(c.contract.id.clone(), (p, c));
        }
    }

    let mut candidates: Vec<ZcCandidate> = Vec::new();

    for (toml_path, c) in by_id.values() {
        let id = c.contract.id.trim();
        if id.is_empty() {
            continue;
        }
        // Skip contracts that already have a ZC suffix.
        if id.ends_with("_zc") {
            continue;
        }

        let idl = c.contract.idl.as_deref().unwrap_or("").trim();
        if idl.is_empty() {
            continue;
        }

        let msg_info = match parse_msg_info(idl) {
            Some(m) => m,
            None => continue, // skip .srv-based contracts (not data streams)
        };

        let zc_contract_id = format!("{id}_zc");
        let namespace = id.rsplit_once('/').map(|(ns, _)| ns).unwrap_or(id);

        candidates.push(ZcCandidate {
            contract_id: id.to_string(),
            zc_contract_id,
            idl_path: idl.to_string(),
            msg_info,
            namespace: namespace.to_string(),
            kind: c.contract.kind.clone().unwrap_or_default(),
            mode: c.mode.mode_type.clone().unwrap_or_default(),
            source_toml: toml_path.clone(),
            description: c
                .contract
                .description
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
        });
    }

    candidates.sort_by(|a, b| a.contract_id.cmp(&b.contract_id));
    Ok(candidates)
}

/// Parse a lib-relative IDL path like
/// `common_interfaces/sensor_msgs/msg/Image.msg` into `ZcMsgInfo`.
/// Returns `None` for `.srv` paths (service definitions, not data streams).
fn parse_msg_info(idl: &str) -> Option<ZcMsgInfo> {
    // Strip .msg extension
    let stem = idl.strip_suffix(".msg")?;
    // Find /msg/ segment
    let parts: Vec<&str> = stem.split('/').collect();
    let n = parts.len();
    if n < 2 {
        return None;
    }
    // The segment before the last should be "msg"
    if parts[n - 2] != "msg" {
        return None;
    }
    let ros_pkg = parts[n - 3].to_string();
    let msg_name = parts[n - 1].to_string();
    let python_import = format!("from {ros_pkg}.msg import {msg_name}");
    let python_class = msg_name.clone();
    let label = msg_name.clone();
    Some(ZcMsgInfo {
        ros_pkg,
        msg_name,
        python_import,
        python_class,
        label,
    })
}

// ── provider discovery for --fill ───────────────────────────────────────────

/// One existing provider discovered under a fill source directory.
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    /// Absolute path to the provider Python file.
    pub path: PathBuf,
    /// Provider id, e.g. "tiago_camera".
    pub provider_id: String,
    /// Namespace, e.g. "robonix/primitive/camera".
    pub namespace: String,
}

/// Walk `dirs` for Python files containing `Primitive(` or `Service(` or
/// `Skill(` calls, and extract their id + namespace.
pub fn discover_providers(dirs: &[PathBuf]) -> Result<Vec<ProviderInfo>> {
    let mut providers: Vec<ProviderInfo> = Vec::new();
    for d in dirs {
        walk_for_providers(d, &mut providers)?;
    }
    let mut seen = std::collections::BTreeSet::new();
    providers.retain(|p| seen.insert(p.provider_id.clone()));
    Ok(providers)
}

fn walk_for_providers(dir: &Path, out: &mut Vec<ProviderInfo>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            walk_for_providers(&p, out)?;
        } else if p.extension().and_then(|x| x.to_str()) == Some("py") {
            if let Some(info) = extract_provider_info(&p) {
                out.push(info);
            }
        }
    }
    Ok(())
}

fn extract_provider_info(path: &Path) -> Option<ProviderInfo> {
    let content = fs::read_to_string(path).ok()?;
    let id = extract_string_arg(&content, "id=")?;
    let namespace = extract_string_arg(&content, "namespace=")?;
    Some(ProviderInfo {
        path: path.to_path_buf(),
        provider_id: id,
        namespace,
    })
}

fn extract_string_arg(content: &str, key: &str) -> Option<String> {
    let pos = content.find(key)?;
    let rest = &content[pos + key.len()..];
    let rest = rest.trim();
    if let Some(rest) = rest.strip_prefix('"') {
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    } else if let Some(rest) = rest.strip_prefix('\'') {
        let end = rest.find('\'')?;
        Some(rest[..end].to_string())
    } else {
        let end = rest.find(|c: char| c == ',' || c == ')' || c == '\n' || c == ' ')?;
        let val = rest[..end].trim();
        if val.is_empty() { None } else { Some(val.to_string()) }
    }
}

// ── TOML walk ───────────────────────────────────────────────────────────────

fn collect_tomls(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    collect_tomls_inner(dir, &mut v)?;
    v.sort();
    Ok(v)
}

fn collect_tomls_inner(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        anyhow::bail!("contracts directory does not exist: {}", dir.display());
    }
    for entry in
        fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))?
    {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            if p.file_name().and_then(|s| s.to_str()) == Some("lib") {
                continue;
            }
            collect_tomls_inner(&p, out)?;
        } else if p.extension().and_then(|x| x.to_str()) == Some("toml") {
            out.push(p);
        }
    }
    Ok(())
}
