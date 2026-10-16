// SPDX-License-Identifier: MulanPSL-2.0
// Author: wheatfox <wheatfox17@icloud.com>
//
// dispatch/mcp.rs — dispatch a CapabilityCall over MCP transport.
//
// (MCP-protocol terminology uses "tool" for the server-side callable.
// Robonix-side this is a capability with transport=MCP — same thing,
// different layer. This file just bridges call_tool on the MCP side
// to CapabilityCall on the executor side.)
//

use crate::pb::pilot::{CapabilityCall, CapabilityCallResult};
use rmcp::{ClientHandler, ServiceExt};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

type McpClient = rmcp::service::RunningService<rmcp::RoleClient, CameraWait>;

struct CameraWait {
    call_id: Option<String>,
    activity: watch::Sender<Instant>,
    reference: Mutex<Option<([u64; 3], u64)>>,
}

impl ClientHandler for CameraWait {
    async fn on_logging_message(
        &self,
        params: rmcp::model::LoggingMessageNotificationParam,
        _: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        if params.logger.as_deref() != Some("vlm_verifier.camera")
            || self.call_id.as_deref().is_none()
            || params.data.get("call_id").and_then(|v| v.as_str()) != self.call_id.as_deref()
        {
            return;
        }
        let Some(reading) = params
            .data
            .get("reference")
            .and_then(|v| serde_json::from_value::<[u64; 3]>(v.clone()).ok())
        else {
            return;
        };
        let mut reference = self.reference.lock().expect("camera reference lock");
        let (first, previous) = reference.get_or_insert((reading, reading[0]));
        if reading[1..] == first[1..]
            && reading[0] >= *previous
            && reading[0] - first[0] <= 5_000_000_000
        {
            *previous = reading[0];
            self.activity.send_replace(Instant::now());
        }
    }
}

/// MCP tool name = leaf of contract_id (e.g.
/// `robonix/service/memory/search` → `search`). Servers expose tools by
/// short name; the provider+contract grouping is Robonix-side bookkeeping.
/// TODO: leaf-as-tool-name may collide across providers — wheatfox
fn mcp_tool_name(contract_id: &str) -> &str {
    contract_id
        .rsplit_once('/')
        .map(|(_, leaf)| leaf)
        .unwrap_or(contract_id)
}

pub async fn execute(call: &CapabilityCall, endpoint: &str) -> CapabilityCallResult {
    let (_cancel_on_drop, cancel) = oneshot::channel();
    execute_with_timeout(call, endpoint, None, cancel).await
}

pub async fn execute_with_timeout(
    call: &CapabilityCall,
    endpoint: &str,
    timeout: Option<Duration>,
    cancel: oneshot::Receiver<()>,
) -> CapabilityCallResult {
    let name = mcp_tool_name(&call.contract_id);
    let result = call_mcp(name, &call.args_json, endpoint, timeout, cancel).await;
    let mut out = CapabilityCallResult {
        call_id: call.call_id.clone(),
        provider_id: call.provider_id.clone(),
        contract_id: call.contract_id.clone(),
        ..Default::default()
    };
    match result {
        Ok(s) => {
            out.success = true;
            out.output = s;
        }
        Err(e) => {
            out.success = false;
            out.error = e.to_string();
        }
    }
    out
}

async fn call_mcp(
    name: &str,
    args_json: &str,
    endpoint: &str,
    timeout: Option<Duration>,
    mut cancel: oneshot::Receiver<()>,
) -> anyhow::Result<String> {
    let args_val: serde_json::Value = serde_json::from_str(args_json)
        .map_err(|e| anyhow::anyhow!("invalid tool arguments JSON: {e}"))?;
    let args_obj = args_val.as_object().cloned();
    let (activity, mut activity_rx) = watch::channel(Instant::now());
    let handler = CameraWait {
        call_id: (name == "verify" && timeout.is_some())
            .then(|| {
                args_val
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
            .flatten(),
        activity,
        reference: Mutex::new(None),
    };
    let mut client = tokio::select! {
        result = tokio::time::timeout(Duration::from_secs(10), connect_mcp(endpoint, handler)) => result??,
        _ = &mut cancel => anyhow::bail!("MCP call canceled"),
    };

    let request = client
        .send_request_with_option(
            rmcp::model::ClientRequest::CallToolRequest(rmcp::model::CallToolRequest::new(
                rmcp::model::CallToolRequestParams::new(name.to_string()).with_arguments(
                    args_obj
                        .map(rmcp::model::JsonObject::from_iter)
                        .unwrap_or_default(),
                ),
            )),
            rmcp::service::PeerRequestOptions::default(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("MCP call_tool failed: {e}"))?;
    let request_id = request.id.clone();
    let response = request.await_response();
    tokio::pin!(response);
    let wait = tokio::time::sleep(timeout.unwrap_or(Duration::from_secs(60)));
    tokio::pin!(wait);
    let result = loop {
        tokio::select! {
            result = &mut response => break result.map_err(anyhow::Error::from),
            _ = &mut cancel => break Err(anyhow::anyhow!("MCP call canceled")),
            _ = &mut wait, if timeout.is_some() => {
                break Err(anyhow::anyhow!("MCP call timed out after {}s without a camera wait checkpoint", timeout.unwrap().as_secs()));
            }
            changed = activity_rx.changed(), if timeout.is_some() => {
                if changed.is_ok() {
                    wait.as_mut().reset(*activity_rx.borrow_and_update() + timeout.unwrap());
                }
            }
        }
    };
    if result.is_err() {
        let _ = tokio::time::timeout(
            Duration::from_secs(2),
            client.notify_cancelled(rmcp::model::CancelledNotificationParam {
                request_id,
                reason: Some("MCP caller stopped waiting".to_owned()),
            }),
        )
        .await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), client.close()).await;
    let rmcp::model::ServerResult::CallToolResult(result) = result? else {
        anyhow::bail!("MCP returned an unexpected response");
    };

    let text: String = result
        .content
        .iter()
        .filter_map(|c| match &c.raw {
            rmcp::model::RawContent::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let is_error = result.is_error.unwrap_or(false);

    if is_error {
        anyhow::bail!("mcp tool error: {}", text);
    }
    Ok(text)
}

async fn connect_mcp(endpoint: &str, handler: CameraWait) -> anyhow::Result<McpClient> {
    if let Some(cmd_str) = endpoint.strip_prefix("stdio://") {
        let mut parts = cmd_str.split_whitespace();
        let program = parts.next().unwrap_or("python3");
        let args: Vec<&str> = parts.collect();
        let mut command = tokio::process::Command::new(program);
        command.args(&args);
        let transport = rmcp::transport::TokioChildProcess::new(command)
            .map_err(|e| anyhow::anyhow!("MCP stdio spawn failed: {e}"))?;
        return handler
            .serve(transport)
            .await
            .map_err(|e| anyhow::anyhow!("MCP init failed: {e}"));
    }
    let base = if endpoint.starts_with("http") {
        endpoint.to_string()
    } else {
        format!("http://{}", endpoint)
    };
    let uri = if base.contains("/mcp") {
        base
    } else {
        format!("{}/mcp", base.trim_end_matches('/'))
    };
    let transport =
        rmcp::transport::streamable_http_client::StreamableHttpClientTransport::from_uri(
            uri.clone(),
        );
    handler
        .serve(transport)
        .await
        .map_err(|e| anyhow::anyhow!("MCP HTTP connect to '{uri}' failed: {e}"))
}
