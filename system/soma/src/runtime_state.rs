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
    arms: BTreeMap<String, (JointSample, (i64, u64))>,
    chassis: BTreeMap<String, (OdomSample, (i64, u64))>,
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

    /// Keep sample ages in the selected reference generation.
    pub async fn apply_line(&self, line: &str) -> anyhow::Result<()> {
        let event: MonitorEvent = serde_json::from_str(line)?;
        let mut state = self.inner.write().await;
        match event {
            MonitorEvent::Ready { subscriptions } => {
                let _ = subscriptions;
            }
            MonitorEvent::JointState(sample) => {
                let received = reference_receipt()?;
                state
                    .arms
                    .insert(sample.provider_id.clone(), (sample, received));
            }
            MonitorEvent::Odom(sample) => {
                let received = reference_receipt()?;
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

    /// Discard observations from a previous simulation run before evaluating freshness.
    pub async fn snapshot(&self) -> anyhow::Result<RuntimeSnapshot> {
        let mut state = self.inner.write().await;
        let now = now_unix()?;
        let (reference, generation) = reference_receipt()?;
        state
            .arms
            .retain(|_, (_, received)| received.1 == generation);
        state
            .chassis
            .retain(|_, (_, received)| received.1 == generation);
        let mut warnings = state.warnings.clone();
        let arms: Vec<ArmSnapshot> = state
            .arms
            .values()
            .map(|(sample, received)| {
                let age_sec = age_seconds(reference, received.0)?;
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
                let age_sec = age_seconds(reference, received.0)?;
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
    Ok(robonix_chronos_clock::wall_timestamp_ns()? as f64 / 1_000_000_000.0)
}

fn reference_receipt() -> anyhow::Result<(i64, u64)> {
    let reference = robonix_chronos_clock::reference_time()?;
    anyhow::ensure!(
        reference.available && reference.timestamp_ns >= 0,
        "Chronos reference unavailable: {}",
        reference.reason
    );
    Ok((reference.timestamp_ns, reference.generation))
}

fn age_seconds(now: i64, received: i64) -> anyhow::Result<f64> {
    let elapsed = now
        .checked_sub(received)
        .filter(|elapsed| *elapsed >= 0)
        .ok_or_else(|| {
            anyhow::anyhow!("Chronos reference time regressed without a generation change")
        })?;
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
    fn reference_read_failures_preserve_runtime_samples() {
        clock_faults::run_case(
            "runtime_state::tests::reference_read_failures_preserve_runtime_samples",
            || {
                tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
                    let store = RuntimeStateStore::new(vec![]);
                    let line = r#"{"kind":"odom","provider_id":"chassis","received_at_unix":1,"linear":[1,0,0]}"#;
                    store.apply_line(line).await.unwrap();
                    let receipt = store.inner.read().await.chassis["chassis"].1;
                    for mode in [1, 2, 4, 16] {
                        clock_faults::mode(mode);
                        assert!(store.apply_line(line).await.is_err());
                        assert_eq!(store.inner.read().await.chassis["chassis"].1, receipt);
                    }
                    clock_faults::mode(0);
                    store.apply_line(line).await.unwrap();
                    store.inner.write().await.chassis.get_mut("chassis").unwrap().1.0 -= 3_000_000_000;
                    let snapshot = store.snapshot().await.unwrap();
                    assert!(!snapshot.chassis[0].fresh && !snapshot.chassis[0].moving);
                });
            },
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn reference_time_controls_freshness_and_reset_discards_samples() {
        clock_faults::run_case(
            "runtime_state::tests::reference_time_controls_freshness_and_reset_discards_samples",
            || {
                use robonix_chronos_clock::{
                    ReferenceConfig, ReferenceSourceKind, VirtualReferenceTime,
                    configure_reference, update_reference_time,
                };
                configure_reference(ReferenceConfig {
                    kind: ReferenceSourceKind::RosTime,
                    ..Default::default()
                })
                .unwrap();
                let publish = |timestamp_ns, generation| {
                    update_reference_time(VirtualReferenceTime {
                        timestamp_ns,
                        generation,
                        generation_changed: false,
                        uncertainty_ns: 1,
                        available: true,
                        verified: false,
                        reason: "ROS time".into(),
                    })
                    .unwrap()
                };
                tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
                    let store = RuntimeStateStore::new(vec![gripper()]);
                    let odom = r#"{"kind":"odom","provider_id":"chassis","received_at_unix":1,"linear":[1,0,0]}"#;
                    assert!(store.apply_line(odom).await.is_err());
                    publish(0, 10);
                    store.apply_line(odom).await.unwrap();
                    store.apply_line(r#"{"kind":"joint_state","provider_id":"piper_ctl","received_at_unix":1,"names":["gripper"],"positions":[0.03]}"#).await.unwrap();
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    assert_eq!(store.snapshot().await.unwrap().chassis[0].age_sec, 0.0);
                    publish(1_000_000_000, 10);
                    assert!(store.snapshot().await.unwrap().chassis[0].fresh);
                    publish(2_000_000_001, 10);
                    let expired = store.snapshot().await.unwrap();
                    assert!(!expired.chassis[0].fresh && !expired.chassis[0].moving);
                    assert_eq!(expired.arms[0].grippers[0].state, "unknown_stale");
                    publish(0, 11);
                    let reset = store.snapshot().await.unwrap();
                    assert!(reset.chassis.is_empty() && reset.arms.is_empty());
                    store.apply_line(odom).await.unwrap();
                    assert!(store.snapshot().await.unwrap().chassis[0].fresh);
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
