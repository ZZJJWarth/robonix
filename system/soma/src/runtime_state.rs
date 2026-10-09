// SPDX-License-Identifier: MulanPSL-2.0

use crate::store::GripperConfig;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub const FRESHNESS_SECS: f64 = 2.0;

#[derive(Clone, Debug, Default)]
pub struct RuntimeStateStore {
    inner: Arc<RwLock<RuntimeState>>,
    grippers: Arc<Vec<GripperConfig>>,
}

#[derive(Clone, Debug, Default)]
struct RuntimeState {
    arms: BTreeMap<String, (JointSample, u64)>,
    chassis: BTreeMap<String, (OdomSample, u64)>,
    warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct JointSample {
    pub provider_id: String,
    pub received_at_unix: f64,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub positions: Vec<f64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OdomSample {
    pub provider_id: String,
    pub received_at_unix: f64,
    #[serde(default)]
    pub linear: [f64; 3],
    #[serde(default)]
    pub angular: [f64; 3],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum MonitorEvent {
    Ready { subscriptions: usize },
    JointState(JointSample),
    Odom(OdomSample),
    Warning { message: String },
}

#[derive(Clone, Debug)]
pub struct RuntimeSnapshot {
    pub observed_at_unix: f64,
    pub arms: Vec<ArmSnapshot>,
    pub chassis: Vec<ChassisSnapshot>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ArmSnapshot {
    pub provider_id: String,
    pub names: Vec<String>,
    pub positions: Vec<f64>,
    pub grippers: Vec<GripperSnapshot>,
    pub fresh: bool,
    pub age_sec: f64,
}

#[derive(Clone, Debug)]
pub struct GripperSnapshot {
    pub config: GripperConfig,
    pub position_m: f64,
    pub likely_holding: bool,
    pub state: String,
    pub fresh: bool,
    pub age_sec: f64,
}

#[derive(Clone, Debug)]
pub struct ChassisSnapshot {
    pub sample: OdomSample,
    pub moving: bool,
    pub fresh: bool,
    pub age_sec: f64,
}

impl RuntimeStateStore {
    pub fn new(grippers: Vec<GripperConfig>) -> Self {
        Self {
            inner: Arc::default(),
            grippers: Arc::new(grippers),
        }
    }

    /// Decode an event and record local steady receipt time before storing samples.
    pub async fn apply_line(&self, line: &str) -> anyhow::Result<()> {
        let event: MonitorEvent = serde_json::from_str(line)?;
        let mut state = self.inner.write().await;
        match event {
            MonitorEvent::Ready { subscriptions } => {
                let _ = subscriptions;
            }
            MonitorEvent::JointState(sample) => {
                let received = robonix_chronos_clock::steady_time()?.timestamp_ns;
                state
                    .arms
                    .insert(sample.provider_id.clone(), (sample, received));
            }
            MonitorEvent::Odom(sample) => {
                let received = robonix_chronos_clock::steady_time()?.timestamp_ns;
                state
                    .chassis
                    .insert(sample.provider_id.clone(), (sample, received));
            }
            MonitorEvent::Warning { message } => {
                if !state.warnings.contains(&message) {
                    state.warnings.push(message);
                    state.warnings.truncate(32);
                }
            }
        }
        Ok(())
    }

    /// Read reference time for publication and steady sample ages without using source stamps.
    pub async fn snapshot(&self) -> anyhow::Result<RuntimeSnapshot> {
        let state = self.inner.read().await;
        let now = now_unix()?;
        let steady = robonix_chronos_clock::steady_time()?.timestamp_ns;
        let mut warnings = state.warnings.clone();
        let arms: Vec<ArmSnapshot> = state
            .arms
            .values()
            .map(|(sample, received)| {
                let age_sec = age_seconds(steady, *received)?;
                let fresh = age_sec <= FRESHNESS_SECS;
                let grippers = self
                    .grippers
                    .iter()
                    .filter(|cfg| cfg.provider_id == sample.provider_id)
                    .map(|cfg| {
                        let position = sample
                            .names
                            .iter()
                            .position(|name| name == &cfg.joint_name)
                            .and_then(|index| sample.positions.get(index).copied());
                        match position {
                            Some(position_m) if fresh => {
                                let open = (position_m - cfg.open_position_m).abs()
                                    <= cfg.open_tolerance_m;
                                GripperSnapshot {
                                    config: cfg.clone(),
                                    position_m,
                                    likely_holding: !open,
                                    state: if open {
                                        "open"
                                    } else {
                                        "holding_or_partially_closed"
                                    }
                                    .to_string(),
                                    fresh,
                                    age_sec,
                                }
                            }
                            Some(position_m) => GripperSnapshot {
                                config: cfg.clone(),
                                position_m,
                                likely_holding: false,
                                state: "unknown_stale".into(),
                                fresh: false,
                                age_sec,
                            },
                            None => GripperSnapshot {
                                config: cfg.clone(),
                                position_m: 0.0,
                                likely_holding: false,
                                state: "unknown_missing_joint".into(),
                                fresh: false,
                                age_sec,
                            },
                        }
                    })
                    .collect();
                Ok(ArmSnapshot {
                    provider_id: sample.provider_id.clone(),
                    names: sample.names.clone(),
                    positions: sample.positions.clone(),
                    grippers,
                    fresh,
                    age_sec,
                })
            })
            .collect::<anyhow::Result<_>>()?;
        let chassis: Vec<ChassisSnapshot> = state
            .chassis
            .values()
            .cloned()
            .map(|(sample, received)| {
                let age_sec = age_seconds(steady, received)?;
                let fresh = age_sec <= FRESHNESS_SECS;
                let linear_speed = sample.linear.iter().map(|v| v * v).sum::<f64>().sqrt();
                let angular_speed = sample.angular.iter().map(|v| v * v).sum::<f64>().sqrt();
                Ok(ChassisSnapshot {
                    moving: fresh && (linear_speed > 0.02 || angular_speed > 0.03),
                    sample,
                    fresh,
                    age_sec,
                })
            })
            .collect::<anyhow::Result<_>>()?;
        if arms.is_empty() {
            warnings.push("no arm joint-state sample is available".into());
        }
        if chassis.is_empty() {
            warnings.push("no chassis odometry sample is available".into());
        }
        Ok(RuntimeSnapshot {
            observed_at_unix: now,
            arms,
            chassis,
            warnings,
        })
    }
}

fn now_unix() -> std::io::Result<f64> {
    Ok(robonix_chronos_clock::reference_timestamp_ns()? as f64 / 1_000_000_000.0)
}

/// Reject steady-clock regression rather than marking a sample fresh.
fn age_seconds(now: u64, received: u64) -> anyhow::Result<f64> {
    let elapsed = now
        .checked_sub(received)
        .ok_or_else(|| anyhow::anyhow!("Chronos steady time regressed"))?;
    Ok(elapsed as f64 / 1_000_000_000.0)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    mod clock_faults {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../chronos/clock/tests/support/mod.rs"
        ));
    }

    #[test]
    #[cfg(target_os = "linux")]
    /// Reference steps do not affect sample ages; failed steady receipts do not replace data.
    fn freshness_uses_local_steady_receipt() {
        clock_faults::run_case(
            "runtime_state::tests::freshness_uses_local_steady_receipt",
            || {
                tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
                let store = RuntimeStateStore::new(vec![]);
                let line = r#"{"kind":"odom","provider_id":"chassis","received_at_unix":1,"linear":[1,0,0]}"#;
                store.apply_line(line).await.unwrap();
                for mode in [8, 32, 0] {
                    clock_faults::mode(mode);
                    let snapshot = store.snapshot().await.unwrap();
                    assert!(snapshot.chassis[0].fresh && snapshot.chassis[0].moving);
                    assert!(snapshot.chassis[0].age_sec < 1.0);
                }
                for mode in [1, 2, 4] {
                    clock_faults::mode(mode);
                    assert!(store.snapshot().await.is_err());
                }
                clock_faults::mode(0);
                let receipt = store.inner.read().await.chassis["chassis"].1;
                clock_faults::mode(16);
                assert!(store.apply_line(line).await.is_err());
                assert_eq!(store.inner.read().await.chassis["chassis"].1, receipt);
                store.inner.write().await.chassis.get_mut("chassis").unwrap().1 -= 3_000_000_000;
                let snapshot = store.snapshot().await.unwrap();
                assert!(!snapshot.chassis[0].fresh && !snapshot.chassis[0].moving);
            });
            },
        );
    }

    use super::*;

    fn gripper() -> GripperConfig {
        GripperConfig {
            component_id: "gripper".into(),
            provider_id: "piper_ctl".into(),
            joint_name: "gripper".into(),
            open_position_m: 0.06937,
            open_tolerance_m: 0.002,
        }
    }

    #[tokio::test]
    async fn calibrated_open_gripper_is_not_reported_as_holding() {
        let store = RuntimeStateStore::new(vec![gripper()]);
        let now = now_unix().unwrap();
        store
            .apply_line(&format!(
                r#"{{"kind":"joint_state","provider_id":"piper_ctl","received_at_unix":{now},"names":["joint1","gripper"],"positions":[0.0,0.06937]}}"#
            ))
            .await
            .unwrap();
        let snapshot = store.snapshot().await.unwrap();
        assert_eq!(snapshot.arms[0].grippers[0].state, "open");
        assert!(!snapshot.arms[0].grippers[0].likely_holding);
    }

    #[tokio::test]
    async fn closed_gripper_is_reported_as_likely_holding() {
        let store = RuntimeStateStore::new(vec![gripper()]);
        let now = now_unix().unwrap();
        store
            .apply_line(&format!(
                r#"{{"kind":"joint_state","provider_id":"piper_ctl","received_at_unix":{now},"names":["gripper"],"positions":[0.03]}}"#
            ))
            .await
            .unwrap();
        let snapshot = store.snapshot().await.unwrap();
        assert!(snapshot.arms[0].grippers[0].likely_holding);
    }

    #[tokio::test]
    async fn chassis_velocity_drives_motion_state() {
        let store = RuntimeStateStore::new(vec![]);
        let now = now_unix().unwrap();
        store
            .apply_line(&format!(
                r#"{{"kind":"odom","provider_id":"ranger_chassis","received_at_unix":{now},"linear":[0.2,0.0,0.0],"angular":[0.0,0.0,0.0]}}"#
            ))
            .await
            .unwrap();
        assert!(store.snapshot().await.unwrap().chassis[0].moving);
    }
}
