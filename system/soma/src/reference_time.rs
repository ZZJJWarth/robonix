// SPDX-License-Identifier: MulanPSL-2.0

use anyhow::{Context, Result};
use robonix_atlas::{client::AtlasClient, pb as atlas_pb};
use robonix_chronos_clock::{VirtualReferenceTime, update_reference_time};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{Duration, timeout},
};

use crate::pb::contracts::robonix_system_chronos_reference_time_client::RobonixSystemChronosReferenceTimeClient;

const CONTRACT: &str = "robonix/system/chronos/reference_time";

pub struct ReferenceSubscription {
    stopped: watch::Sender<bool>,
    task: Option<JoinHandle<Result<()>>>,
}

impl ReferenceSubscription {
    pub fn start(mut atlas: AtlasClient, consumer: String, provider: String) -> Self {
        let (stopped, mut stop) = watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                let cleanup = timeout(
                    Duration::from_secs(2),
                    release_channels(&mut atlas, &consumer, &provider),
                )
                .await
                .context("reference channel cleanup timed out")
                .and_then(|result| result);
                if let Err(error) = cleanup {
                    invalidate("Chronos reference channel cleanup failed")?;
                    if *stop.borrow() {
                        return Err(error);
                    }
                    robonix_scribe::warn!("[soma/reference] cleanup failed: {error:#}");
                } else {
                    if *stop.borrow() {
                        break;
                    }
                    // Finish bounded Connect before honoring cancellation so cleanup can find its outcome.
                    let connected = timeout(
                        Duration::from_secs(2),
                        atlas.connect_capability(
                            &consumer,
                            &provider,
                            CONTRACT,
                            atlas_pb::Transport::Grpc,
                        ),
                    )
                    .await;
                    let result = match connected {
                        Ok(Ok((_edge, endpoint, _))) if !*stop.borrow() => tokio::select! {
                            _ = stop.changed() => Ok(()),
                            result = receive(&endpoint) => result,
                        },
                        Ok(Ok(_)) => Ok(()),
                        Ok(Err(error)) => Err(error),
                        Err(error) => Err(error.into()),
                    };
                    invalidate("Chronos reference stream is disconnected")?;
                    if let Err(error) = result {
                        robonix_scribe::warn!("[soma/reference] {error:#}");
                    }
                    if *stop.borrow() {
                        continue;
                    }
                }
                tokio::select! {
                    _ = stop.changed() => {},
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
            invalidate("Soma reference subscription stopped")
        });
        Self {
            stopped,
            task: Some(task),
        }
    }

    pub async fn stop(mut self) -> Result<()> {
        self.stopped.send_replace(true);
        self.task
            .take()
            .expect("owned reference subscription")
            .await
            .context("join reference subscription")?
    }
}

impl Drop for ReferenceSubscription {
    fn drop(&mut self) {
        self.stopped.send_replace(true);
    }
}

async fn receive(endpoint: &str) -> Result<()> {
    let address = if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_owned()
    } else {
        format!("http://{endpoint}")
    };
    let mut client = timeout(
        Duration::from_secs(2),
        RobonixSystemChronosReferenceTimeClient::connect(address),
    )
    .await??;
    let mut stream = timeout(Duration::from_secs(2), client.reference_time(()))
        .await??
        .into_inner();
    loop {
        // Publications continue during a simulation pause; this bounds transport failure only.
        let reading = timeout(Duration::from_secs(5), stream.message())
            .await
            .context("Chronos reference stream stopped responding")??
            .context("Chronos reference stream ended")?;
        update_reference_time(VirtualReferenceTime {
            timestamp_ns: reading.timestamp_ns,
            generation: reading.generation,
            generation_changed: reading.generation_changed,
            uncertainty_ns: reading.uncertainty_ns,
            available: reading.available,
            verified: reading.verified,
            reason: reading.reason,
        })?;
    }
}

fn invalidate(reason: &str) -> Result<()> {
    let mut reference = robonix_chronos_clock::reference_time()?;
    reference.available = false;
    reference.reason = reason.into();
    update_reference_time(reference)?;
    Ok(())
}

// Inspection recovers the channel ID even when an Atlas Connect reply was lost.
async fn release_channels(atlas: &mut AtlasClient, consumer: &str, provider: &str) -> Result<()> {
    let response = atlas
        .inner()
        .inspect_atlas(atlas_pb::InspectAtlasRequest {})
        .await?;
    let inspected: serde_json::Value = serde_json::from_str(&response.into_inner().json)?;
    let channels = inspected["channels"]
        .as_object()
        .context("Atlas inspection has no channels object")?;
    for (id, channel) in channels {
        if channel["consumer_id"].as_str() == Some(consumer)
            && channel["provider_id"].as_str() == Some(provider)
            && channel["contract_id"].as_str() == Some(CONTRACT)
            && channel["transport"].as_str() == Some(atlas_pb::Transport::Grpc.as_str_name())
        {
            atlas.disconnect_capability(id).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use robonix_atlas::{
        client,
        service::{AtlasRegistry, serve_atlas},
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn lost_connect_replies_are_released_without_touching_other_consumers() {
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserved.local_addr().unwrap();
        drop(reserved);
        let registry = Arc::new(AtlasRegistry::default());
        let server = tokio::spawn(serve_atlas(registry.clone(), address));
        let mut atlas =
            AtlasClient::connect_with_retry(address.to_string(), 50, Duration::from_millis(10))
                .await
                .unwrap();
        for id in ["soma", "other", "chronos"] {
            atlas
                .register_service(id, "robonix/system/chronos", "")
                .await
                .unwrap();
        }
        for (contract, path, service, method) in [
            (
                "robonix/lifecycle/driver",
                "capabilities/lifecycle/driver.v1.toml",
                "RobonixLifecycleDriver",
                "Driver",
            ),
            (
                CONTRACT,
                "capabilities/system/chronos/reference_time.v1.toml",
                "RobonixSystemChronosReferenceTime",
                "ReferenceTime",
            ),
        ] {
            atlas
                .declare_capability(
                    "chronos",
                    contract,
                    atlas_pb::Transport::Grpc,
                    "127.0.0.1:1",
                    client::grpc_params(
                        path,
                        format!("robonix.contracts.{service}"),
                        format!("/robonix.contracts.{service}/{method}"),
                    ),
                )
                .await
                .unwrap();
        }
        atlas
            .set_lifecycle_state("chronos", atlas_pb::LifecycleState::StateInactive, "")
            .await
            .unwrap();
        atlas
            .set_lifecycle_state("chronos", atlas_pb::LifecycleState::StateActive, "")
            .await
            .unwrap();
        let (other, _, _) = atlas
            .connect_capability("other", "chronos", CONTRACT, atlas_pb::Transport::Grpc)
            .await
            .unwrap();
        for _ in 0..3 {
            let mut connection = atlas.clone();
            // Remote allocation completes, but the application loses the reply before obtaining its ID.
            assert!(
                timeout(Duration::from_millis(30), async {
                    let edge = connection
                        .connect_capability("soma", "chronos", CONTRACT, atlas_pb::Transport::Grpc)
                        .await
                        .unwrap();
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    edge
                })
                .await
                .is_err()
            );
            release_channels(&mut atlas, "soma", "chronos")
                .await
                .unwrap();
            let inspected: serde_json::Value =
                serde_json::from_str(&registry.inspect_json().await.unwrap()).unwrap();
            assert_eq!(inspected["channels"].as_object().unwrap().len(), 1);
            assert!(inspected["channels"].get(&other).is_some());
        }
        atlas.disconnect_capability(&other).await.unwrap();
        server.abort();
    }
}
